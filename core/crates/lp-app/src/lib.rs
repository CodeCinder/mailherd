//! The LanternPost app core: state, view models and actions.
//!
//! Every UI talks only to [`Core`]. It asks for view models (sidebar, thread
//! list, People list, conversation) and sends actions (open, pin, done,
//! reply, undo send). Provider-specific work happens behind [`Transport`].

use std::collections::{HashMap, HashSet};

use lp_chat::{ReplyParts, learn_signature, snippet, split_reply, strip_learned_signature};
use lp_mime::{
    OutgoingMessage, QuotedOriginal, build_message, html_to_text, parse_message, reply_subject,
};
use lp_model::{
    Account, AccountId, Address, Flags, MessageHeader, MessageId, ProviderKind, ThreadState,
    Timestamp, normalize_email,
};
use lp_store::{FolderRole, NewMessage, Store, StoreError};
use lp_thread::{Thread, build_threads, people_key};
use serde::{Deserialize, Serialize};

pub mod demo;
pub mod format;
pub mod view;

use format::{Clock, avatar_color, initials, names_label, size_label};
use view::{
    AccountRow, AttachmentRow, Avatar, Bubble, Conversation, ConversationItem, DeliveryState,
    Mailbox, PendingSend, PersonRow, Sidebar, SidebarItem, ThreadRow,
};

/// Default length of the undo-send window.
pub const UNDO_SEND_SECONDS: i64 = 10;
const SNIPPET_CHARS: usize = 140;
const MAX_REFERENCES: usize = 20;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("could not read the message: {0}")]
    Parse(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("could not build the message: {0}")]
    Build(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;

/// Changes the core asks the provider to make, queued in the outbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RemoteAction {
    MarkRead {
        provider_ids: Vec<String>,
    },
    MarkUnread {
        provider_ids: Vec<String>,
    },
    Archive {
        provider_ids: Vec<String>,
    },
    MoveToInbox {
        provider_ids: Vec<String>,
    },
    SetThreadState {
        thread_id: String,
        state: ThreadState,
    },
}

/// How the core reaches mail providers. Implemented per provider by the sync layer,
/// and by [`demo::DemoTransport`] for the offline demo.
pub trait Transport: Send {
    fn send(&mut self, account: &Account, raw: &[u8]) -> std::result::Result<(), String>;
    fn apply(
        &mut self,
        account: &Account,
        action: &RemoteAction,
    ) -> std::result::Result<(), String>;
}

/// A raw message arriving from a provider.
#[derive(Debug, Clone)]
pub struct Incoming {
    pub account_id: AccountId,
    pub provider_id: String,
    pub provider_thread_id: Option<String>,
    pub raw: Vec<u8>,
    pub folder_ids: Vec<i64>,
    pub flags: Flags,
    /// Fallback date when the message has no Date header (the server's received time).
    pub received_at: Timestamp,
}

#[derive(Debug, Serialize, Deserialize)]
struct SendPayload {
    thread_id: String,
    raw: String,
    text: String,
    date: Timestamp,
}

const SEND_KIND: &str = "send";
const ACTION_KIND: &str = "action";

/// Everything derived from the store that the views need, rebuilt after each change.
#[derive(Default)]
struct Cache {
    headers: Vec<MessageHeader>,
    threads: Vec<Thread>,
    thread_by_id: HashMap<String, usize>,
    roles: HashMap<MessageId, Vec<FolderRole>>,
    states: HashMap<String, ThreadState>,
}

pub struct Core {
    store: Store,
    accounts: Vec<Account>,
    me: HashSet<String>,
    transport: Box<dyn Transport>,
    now: Box<dyn Fn() -> Timestamp + Send>,
    utc_offset: i32,
    undo_seconds: i64,
    cache: Cache,
}

fn hex_color(color: u32) -> String {
    format!("#{:06x}", color & 0x00ff_ffff)
}

fn provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::ICloud => "iCloud",
        ProviderKind::Gmail => "Gmail",
        ProviderKind::Microsoft365 => "Microsoft 365",
    }
}

fn dedupe(addresses: Vec<Address>) -> Vec<Address> {
    let mut seen = HashSet::new();
    addresses
        .into_iter()
        .filter(|a| seen.insert(a.normalized()))
        .collect()
}

impl Core {
    pub fn new(
        store: Store,
        transport: Box<dyn Transport>,
        now: Box<dyn Fn() -> Timestamp + Send>,
        utc_offset_seconds: i32,
    ) -> Result<Self> {
        let mut core = Self {
            store,
            accounts: Vec::new(),
            me: HashSet::new(),
            transport,
            now,
            utc_offset: utc_offset_seconds,
            undo_seconds: UNDO_SEND_SECONDS,
            cache: Cache::default(),
        };
        core.reload()?;
        Ok(core)
    }

    pub fn set_utc_offset(&mut self, seconds: i32) {
        self.utc_offset = seconds;
    }

    pub fn set_undo_seconds(&mut self, seconds: i64) {
        self.undo_seconds = seconds.clamp(0, 60);
    }

    fn now(&self) -> Timestamp {
        (self.now)()
    }

    fn clock(&self) -> Clock {
        Clock::new(self.now(), self.utc_offset)
    }

    fn reload(&mut self) -> Result<()> {
        self.accounts = self.store.accounts()?;
        self.me = self
            .accounts
            .iter()
            .map(|a| normalize_email(&a.email))
            .collect();
        let headers = self.store.all_headers()?;
        let threads = build_threads(&headers);
        let thread_by_id = threads
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();
        let mut states = HashMap::new();
        for t in &threads {
            let state = self.store.thread_state(&t.id)?;
            if state != ThreadState::default() {
                states.insert(t.id.clone(), state);
            }
        }
        self.cache = Cache {
            roles: self.store.message_roles()?,
            headers,
            threads,
            thread_by_id,
            states,
        };
        Ok(())
    }

    fn is_me(&self, address: &Address) -> bool {
        self.me.contains(&address.normalized())
    }

    fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    fn account_color(&self, id: &str) -> String {
        self.account(id)
            .map_or_else(|| "#888888".to_string(), |a| hex_color(a.color))
    }

    fn avatar(&self, address: &Address) -> Avatar {
        Avatar {
            initials: initials(address),
            color: hex_color(avatar_color(&address.email)),
        }
    }

    fn state(&self, thread_id: &str) -> ThreadState {
        self.cache
            .states
            .get(thread_id)
            .copied()
            .unwrap_or_default()
    }

    // ---- accounts and ingest ---------------------------------------------

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn add_account(&mut self, account: &Account) -> Result<()> {
        self.store.upsert_account(account)?;
        self.reload()
    }

    pub fn upsert_folder(
        &mut self,
        account_id: &str,
        provider_id: &str,
        name: &str,
        role: Option<FolderRole>,
    ) -> Result<i64> {
        Ok(self
            .store
            .upsert_folder(account_id, provider_id, name, role)?)
    }

    /// Store a message from a provider. Call [`Core::refresh`] after a batch.
    pub fn ingest(&mut self, incoming: &Incoming) -> Result<MessageId> {
        let parsed = parse_message(&incoming.raw).map_err(|e| CoreError::Parse(e.to_string()))?;
        let text = parsed
            .body
            .text
            .clone()
            .or_else(|| parsed.body.html.as_deref().map(html_to_text))
            .unwrap_or_default();
        let parts = split_reply(&text);
        let from = parsed
            .from
            .clone()
            .unwrap_or_else(|| Address::new(None, "unknown@invalid"));
        let header = MessageHeader {
            id: 0,
            account_id: incoming.account_id.clone(),
            provider_id: incoming.provider_id.clone(),
            provider_thread_id: incoming.provider_thread_id.clone(),
            rfc_message_id: parsed.rfc_message_id.clone(),
            in_reply_to: parsed.in_reply_to.clone(),
            references: parsed.references.clone(),
            subject: parsed.subject.clone(),
            from,
            to: parsed.to.clone(),
            cc: parsed.cc.clone(),
            reply_to: parsed.reply_to.clone(),
            date: parsed.date.unwrap_or(incoming.received_at),
            flags: incoming.flags,
            has_attachments: parsed.attachments.iter().any(|a| !a.inline),
            snippet: snippet(&parts.new_text, SNIPPET_CHARS),
        };
        let id = self.store.upsert_message(&NewMessage {
            header,
            body: parsed.body,
            attachments: parsed.attachments,
            is_bulk: parsed.is_bulk,
            folder_ids: incoming.folder_ids.clone(),
        })?;
        Ok(id)
    }

    /// Rebuild threads and views after ingesting messages.
    pub fn refresh(&mut self) -> Result<()> {
        self.reload()
    }

    // ---- thread membership -----------------------------------------------

    fn copies<'a>(&'a self, thread: &'a Thread) -> impl Iterator<Item = &'a MessageHeader> + 'a {
        thread
            .messages
            .iter()
            .flat_map(|m| m.copies.iter())
            .map(|&i| &self.cache.headers[i])
    }

    fn has_role(&self, thread: &Thread, role: FolderRole, account: Option<&str>) -> bool {
        self.copies(thread).any(|h| {
            account.is_none_or(|a| h.account_id == a)
                && self
                    .cache
                    .roles
                    .get(&h.id)
                    .is_some_and(|r| r.contains(&role))
        })
    }

    fn in_mailbox(&self, thread: &Thread, mailbox: &Mailbox) -> bool {
        let state = self.state(&thread.id);
        let snoozed = state.snoozed_until.is_some_and(|t| t > self.now());
        match mailbox {
            Mailbox::Inbox => !snoozed && self.has_role(thread, FolderRole::Inbox, None),
            Mailbox::AccountInbox(id) => {
                !snoozed && self.has_role(thread, FolderRole::Inbox, Some(id))
            }
            Mailbox::Pinned => state.pinned,
            Mailbox::Done => {
                !self.has_role(thread, FolderRole::Inbox, None)
                    && self.has_role(thread, FolderRole::Archive, None)
            }
            Mailbox::Sent => self.copies(thread).any(|h| self.is_me(&h.from)),
        }
    }

    fn unread(&self, thread: &Thread) -> bool {
        self.copies(thread)
            .any(|h| !h.flags.seen && !self.is_me(&h.from))
    }

    fn threads_in<'a>(&'a self, mailbox: &'a Mailbox) -> impl Iterator<Item = &'a Thread> + 'a {
        self.cache
            .threads
            .iter()
            .filter(move |t| self.in_mailbox(t, mailbox))
    }

    fn thread(&self, id: &str) -> Option<&Thread> {
        self.cache
            .thread_by_id
            .get(id)
            .map(|&i| &self.cache.threads[i])
    }

    // ---- sidebar and lists -------------------------------------------------

    pub fn sidebar(&self) -> Sidebar {
        let count = |mb: &Mailbox| self.threads_in(mb).filter(|t| self.unread(t)).count();
        let item = |mailbox: Mailbox, label: &str, icon: &str, color: Option<String>| SidebarItem {
            unread_count: count(&mailbox),
            mailbox,
            label: label.to_string(),
            icon: icon.to_string(),
            color,
        };
        let mut items = vec![
            item(Mailbox::Inbox, "Inbox", "inbox", None),
            item(Mailbox::Pinned, "Pinned", "pin", None),
            item(Mailbox::Sent, "Sent", "send", None),
            item(Mailbox::Done, "Done", "check", None),
        ];
        let accounts = self
            .accounts
            .iter()
            .map(|a| {
                let mailbox = Mailbox::AccountInbox(a.id.clone());
                items.push(item(
                    mailbox.clone(),
                    &a.email,
                    "account",
                    Some(hex_color(a.color)),
                ));
                AccountRow {
                    id: a.id.clone(),
                    email: a.email.clone(),
                    display_name: a.display_name.clone(),
                    provider: provider_name(a.kind).to_string(),
                    color: hex_color(a.color),
                    unread_count: count(&mailbox),
                }
            })
            .collect();
        Sidebar { items, accounts }
    }

    fn thread_row(&self, thread: &Thread, clock: &Clock) -> Option<ThreadRow> {
        let first = self.cache.headers.get(thread.messages.first()?.index)?;
        let last = self.cache.headers.get(thread.messages.last()?.index)?;
        let mut senders: Vec<String> = Vec::new();
        for m in &thread.messages {
            let h = &self.cache.headers[m.index];
            let name = if self.is_me(&h.from) {
                "Me".to_string()
            } else {
                h.from.display()
            };
            if !senders.contains(&name) {
                senders.push(name);
            }
        }
        let latest_other = thread
            .messages
            .iter()
            .rev()
            .map(|m| &self.cache.headers[m.index])
            .find(|h| !self.is_me(&h.from))
            .unwrap_or(last);
        let mut account_colors: Vec<String> = Vec::new();
        for m in &thread.messages {
            for a in &m.accounts {
                let c = self.account_color(a);
                if !account_colors.contains(&c) {
                    account_colors.push(c);
                }
            }
        }
        let snippet = if self.is_me(&last.from) {
            format!("You: {}", last.snippet)
        } else {
            last.snippet.clone()
        };
        Some(ThreadRow {
            thread_id: thread.id.clone(),
            subject: if first.subject.is_empty() {
                "(no subject)".into()
            } else {
                first.subject.clone()
            },
            participants: names_label(&senders),
            snippet,
            date_label: clock.list_label(last.date),
            timestamp: last.date,
            unread: self.unread(thread),
            message_count: thread.messages.len(),
            pinned: self.state(&thread.id).pinned,
            has_attachments: self.copies(thread).any(|h| h.has_attachments),
            account_colors,
            avatar: self.avatar(&latest_other.from),
            is_bulk: self.store.is_bulk(latest_other.id).unwrap_or(false),
        })
    }

    /// Threads in a mailbox, pinned first, then newest first.
    pub fn thread_list(&self, mailbox: &Mailbox) -> Vec<ThreadRow> {
        let clock = self.clock();
        let mut rows: Vec<ThreadRow> = self
            .threads_in(mailbox)
            .filter_map(|t| self.thread_row(t, &clock))
            .collect();
        rows.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.timestamp.cmp(&a.timestamp)));
        rows
    }

    fn thread_people(&self, thread: &Thread) -> Vec<String> {
        let addresses: Vec<&Address> = self
            .copies(thread)
            .flat_map(|h| {
                std::iter::once(&h.from)
                    .chain(h.to.iter())
                    .chain(h.cc.iter())
            })
            .collect();
        people_key(addresses, &self.me)
    }

    fn display_name_for(&self, email: &str) -> String {
        self.cache
            .headers
            .iter()
            .rev()
            .flat_map(|h| {
                std::iter::once(&h.from)
                    .chain(h.to.iter())
                    .chain(h.cc.iter())
            })
            .find(|a| a.normalized() == email && a.name.is_some())
            .map(Address::display)
            .unwrap_or_else(|| Address::new(None, email).display())
    }

    /// Conversations grouped by who you talk to, newest first.
    pub fn people_list(&self, mailbox: &Mailbox) -> Vec<PersonRow> {
        let clock = self.clock();
        let mut groups: HashMap<String, Vec<&Thread>> = HashMap::new();
        let mut order: Vec<String> = Vec::new();
        for t in self.threads_in(mailbox) {
            let people = self.thread_people(t);
            if people.is_empty() {
                continue;
            }
            let key = people.join(",");
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups.entry(key).or_default().push(t);
        }
        let mut rows: Vec<PersonRow> = order
            .into_iter()
            .filter_map(|key| {
                let threads = groups.get(&key)?;
                let latest = threads
                    .iter()
                    .filter_map(|t| t.last_index().map(|i| &self.cache.headers[i]))
                    .max_by_key(|h| h.date)?;
                let emails: Vec<&str> = key.split(',').collect();
                let names: Vec<String> = emails.iter().map(|e| self.display_name_for(e)).collect();
                let is_group = emails.len() > 1;
                let avatar_address = Address::new(Some(&names[0]), emails[0]);
                let avatar = if is_group {
                    Avatar {
                        initials: format!("{}", emails.len()),
                        color: hex_color(avatar_color(&key)),
                    }
                } else {
                    self.avatar(&avatar_address)
                };
                let snippet = if self.is_me(&latest.from) {
                    format!("You: {}", latest.snippet)
                } else if is_group {
                    format!("{}: {}", latest.from.display(), latest.snippet)
                } else {
                    latest.snippet.clone()
                };
                Some(PersonRow {
                    name: names_label(&names),
                    is_group,
                    snippet,
                    date_label: clock.list_label(latest.date),
                    timestamp: latest.date,
                    unread: threads.iter().any(|t| self.unread(t)),
                    thread_count: threads.len(),
                    pinned: threads.iter().any(|t| self.state(&t.id).pinned),
                    avatar,
                    key,
                })
            })
            .collect();
        rows.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.timestamp.cmp(&a.timestamp)));
        rows
    }

    // ---- conversations ----------------------------------------------------

    fn message_parts(&self, header: &MessageHeader) -> (ReplyParts, bool) {
        let body = self
            .store
            .body(header.id)
            .ok()
            .flatten()
            .unwrap_or_default();
        let has_html = body.html.is_some();
        let text = body
            .text
            .or_else(|| body.html.as_deref().map(html_to_text))
            .unwrap_or_default();
        (split_reply(&text), has_html)
    }

    fn bubbles_for(
        &self,
        entries: &[(&Thread, usize)],
        is_group: bool,
        clock: &Clock,
    ) -> Vec<ConversationItem> {
        // Split every message first so signatures can be learned per sender.
        let mut prepared: Vec<(&MessageHeader, &lp_thread::ThreadMessage, ReplyParts, bool)> =
            Vec::new();
        for (thread, pos) in entries {
            let tm = &thread.messages[*pos];
            let header = &self.cache.headers[tm.index];
            let (parts, has_html) = self.message_parts(header);
            prepared.push((header, tm, parts, has_html));
        }
        let mut by_sender: HashMap<String, Vec<String>> = HashMap::new();
        for (h, _, parts, _) in &prepared {
            by_sender
                .entry(h.from.normalized())
                .or_default()
                .push(parts.new_text.clone());
        }
        let learned: HashMap<String, String> = by_sender
            .into_iter()
            .filter_map(|(sender, texts)| {
                let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                learn_signature(&refs).map(|sig| (sender, sig))
            })
            .collect();

        let mut items = Vec::new();
        let mut last_day = None;
        let mut last_sender: Option<String> = None;
        let mut last_subject: Option<String> = None;
        for (h, tm, mut parts, has_html) in prepared {
            if let Some(sig) = learned.get(&h.from.normalized()) {
                let (text, removed) = strip_learned_signature(&parts.new_text, sig);
                if removed && !text.is_empty() {
                    parts.new_text = text;
                    parts.signature.get_or_insert_with(|| sig.clone());
                }
            }
            let day = clock.day_key(h.date);
            if last_day != Some(day) {
                items.push(ConversationItem::Day {
                    label: clock.day_label(h.date),
                });
                last_day = Some(day);
                last_sender = None;
            }
            let normalized_subject = lp_thread::normalize_subject(&h.subject);
            let subject_change = match &last_subject {
                Some(prev) if *prev != normalized_subject => Some(h.subject.clone()),
                _ => None,
            };
            last_subject = Some(normalized_subject);
            let is_me = self.is_me(&h.from);
            let sender = h.from.normalized();
            let show_sender = is_group && !is_me && last_sender.as_deref() != Some(&sender);
            last_sender = Some(sender);
            let attachments = self
                .store
                .attachments(h.id)
                .unwrap_or_default()
                .into_iter()
                .filter(|a| !a.inline)
                .map(|a| AttachmentRow {
                    is_image: a.mime_type.starts_with("image/"),
                    filename: a.filename.unwrap_or_else(|| "attachment".into()),
                    size_label: size_label(a.size),
                    mime_type: a.mime_type,
                })
                .collect();
            items.push(ConversationItem::Message(Box::new(Bubble {
                message_id: h.id,
                sender_name: if is_me { "Me".into() } else { h.from.display() },
                sender_email: h.from.email.clone(),
                is_me,
                has_hidden: parts.has_hidden(),
                text: parts.new_text,
                quoted: parts.quoted,
                signature: parts.signature,
                forwarded: parts.forwarded,
                time_label: clock.time_label(h.date),
                timestamp: h.date,
                avatar: self.avatar(&h.from),
                attachments,
                is_card: !is_me && self.store.is_bulk(h.id).unwrap_or(false),
                has_html,
                account_colors: tm.accounts.iter().map(|a| self.account_color(a)).collect(),
                delivery: if is_me {
                    DeliveryState::Sent
                } else {
                    DeliveryState::Received
                },
                subject_change,
                show_sender,
            })));
        }
        items
    }

    fn pending_bubbles(&self, thread_ids: &[&str], clock: &Clock) -> Vec<ConversationItem> {
        let Ok(items) = self.store.pending_outbox() else {
            return Vec::new();
        };
        items
            .into_iter()
            .filter(|i| i.kind == SEND_KIND)
            .filter_map(|i| {
                let p: SendPayload = serde_json::from_str(&i.payload).ok()?;
                if !thread_ids.contains(&p.thread_id.as_str()) {
                    return None;
                }
                let account = self.account(&i.account_id)?;
                let me = Address::new(Some(&account.display_name), &account.email);
                Some(ConversationItem::Message(Box::new(Bubble {
                    message_id: -i.id,
                    sender_name: "Me".into(),
                    sender_email: account.email.clone(),
                    is_me: true,
                    text: p.text,
                    quoted: None,
                    signature: None,
                    forwarded: false,
                    has_hidden: false,
                    time_label: clock.time_label(p.date),
                    timestamp: p.date,
                    avatar: self.avatar(&me),
                    attachments: Vec::new(),
                    is_card: false,
                    has_html: false,
                    account_colors: vec![hex_color(account.color)],
                    delivery: if i.attempts > 0 {
                        DeliveryState::Failed
                    } else {
                        DeliveryState::Sending
                    },
                    subject_change: None,
                    show_sender: false,
                })))
            })
            .collect()
    }

    /// Recipients of a reply-all to the latest message of a thread, and the account to send from.
    fn reply_target(
        &self,
        thread: &Thread,
    ) -> Option<(Account, Vec<Address>, Vec<Address>, &MessageHeader)> {
        let tm = thread.messages.last()?;
        let base = &self.cache.headers[tm.index];
        let (to, cc) = if self.is_me(&base.from) {
            (base.to.clone(), base.cc.clone())
        } else {
            let mut to = if base.reply_to.is_empty() {
                vec![base.from.clone()]
            } else {
                base.reply_to.clone()
            };
            to.extend(base.to.iter().cloned());
            (to, base.cc.clone())
        };
        let to: Vec<Address> = dedupe(to).into_iter().filter(|a| !self.is_me(a)).collect();
        let to_set: HashSet<String> = to.iter().map(Address::normalized).collect();
        let cc: Vec<Address> = dedupe(cc)
            .into_iter()
            .filter(|a| !self.is_me(a) && !to_set.contains(&a.normalized()))
            .collect();
        // Reply from the account the message was addressed to, else the one that holds it.
        let addressed: HashSet<String> = base
            .to
            .iter()
            .chain(base.cc.iter())
            .chain(std::iter::once(&base.from))
            .map(Address::normalized)
            .collect();
        let account = self
            .accounts
            .iter()
            .filter(|a| tm.accounts.contains(&a.id))
            .find(|a| addressed.contains(&normalize_email(&a.email)))
            .or_else(|| tm.accounts.first().and_then(|id| self.account(id)))?
            .clone();
        Some((account, to, cc, base))
    }

    fn reply_label(&self, thread: &Thread) -> String {
        match self.reply_target(thread) {
            Some((_, to, cc, _)) => {
                let names: Vec<String> = to.iter().chain(cc.iter()).map(Address::display).collect();
                if names.is_empty() {
                    "To yourself".into()
                } else {
                    format!("To {}", names_label(&names))
                }
            }
            None => String::new(),
        }
    }

    pub fn conversation(&self, thread_id: &str) -> Option<Conversation> {
        let thread = self.thread(thread_id)?;
        let clock = self.clock();
        let people = self.thread_people(thread);
        let is_group = people.len() > 1;
        let entries: Vec<(&Thread, usize)> =
            (0..thread.messages.len()).map(|i| (thread, i)).collect();
        let mut items = self.bubbles_for(&entries, is_group, &clock);
        items.extend(self.pending_bubbles(&[thread_id], &clock));
        let first = &self.cache.headers[thread.messages.first()?.index];
        let names: Vec<String> = people.iter().map(|e| self.display_name_for(e)).collect();
        Some(Conversation {
            id: thread.id.clone(),
            title: if first.subject.is_empty() {
                "(no subject)".into()
            } else {
                first.subject.clone()
            },
            subtitle: names_label(&names),
            is_group,
            pinned: self.state(thread_id).pinned,
            items,
            reply_label: self.reply_label(thread),
        })
    }

    fn person_threads(&self, key: &str) -> Vec<&Thread> {
        let mut threads: Vec<&Thread> = self
            .cache
            .threads
            .iter()
            .filter(|t| self.thread_people(t).join(",") == key)
            .collect();
        threads.sort_by_key(|t| t.last_index().map_or(0, |i| self.cache.headers[i].date));
        threads
    }

    /// Every conversation with one person or group, as one chat.
    pub fn person_conversation(&self, key: &str) -> Option<Conversation> {
        let threads = self.person_threads(key);
        let latest = *threads.last()?;
        let clock = self.clock();
        let mut entries: Vec<(&Thread, usize)> = threads
            .iter()
            .flat_map(|t| (0..t.messages.len()).map(move |i| (*t, i)))
            .collect();
        entries.sort_by_key(|(t, i)| self.cache.headers[t.messages[*i].index].date);
        let emails: Vec<&str> = key.split(',').collect();
        let is_group = emails.len() > 1;
        let mut items = self.bubbles_for(&entries, is_group, &clock);
        let ids: Vec<&str> = threads.iter().map(|t| t.id.as_str()).collect();
        items.extend(self.pending_bubbles(&ids, &clock));
        let names: Vec<String> = emails.iter().map(|e| self.display_name_for(e)).collect();
        Some(Conversation {
            id: format!("people:{key}"),
            title: names_label(&names),
            subtitle: emails.join(", "),
            is_group,
            pinned: threads.iter().any(|t| self.state(&t.id).pinned),
            items,
            reply_label: self.reply_label(latest),
        })
    }

    /// The thread a conversation id refers to; for People conversations, the latest one.
    fn resolve_thread(&self, conversation_id: &str) -> Option<&Thread> {
        match conversation_id.strip_prefix("people:") {
            Some(key) => self.person_threads(key).last().copied(),
            None => self.thread(conversation_id),
        }
    }

    // ---- actions ------------------------------------------------------------

    fn queue_action(&self, account_id: &str, action: &RemoteAction) -> Result<()> {
        let payload = serde_json::to_string(action).map_err(|e| CoreError::Build(e.to_string()))?;
        let now = self.now();
        self.store
            .enqueue(account_id, ACTION_KIND, &payload, now, now)?;
        Ok(())
    }

    fn provider_ids_by_account<'a>(
        &self,
        headers: impl Iterator<Item = &'a MessageHeader>,
    ) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for h in headers {
            map.entry(h.account_id.clone())
                .or_default()
                .push(h.provider_id.clone());
        }
        map
    }

    fn thread_ids_for(&self, conversation_id: &str) -> Vec<String> {
        match conversation_id.strip_prefix("people:") {
            Some(key) => self
                .person_threads(key)
                .iter()
                .map(|t| t.id.clone())
                .collect(),
            None => vec![conversation_id.to_string()],
        }
    }

    /// Mark every unread message in a conversation as read.
    pub fn mark_read(&mut self, conversation_id: &str) -> Result<()> {
        let mut changed: Vec<MessageHeader> = Vec::new();
        for id in self.thread_ids_for(conversation_id) {
            if let Some(thread) = self.thread(&id) {
                changed.extend(self.copies(thread).filter(|h| !h.flags.seen).cloned());
            }
        }
        if changed.is_empty() {
            return Ok(());
        }
        for h in &changed {
            self.store.set_flags(
                h.id,
                Flags {
                    seen: true,
                    ..h.flags
                },
            )?;
        }
        for (account, provider_ids) in self.provider_ids_by_account(changed.iter()) {
            self.queue_action(&account, &RemoteAction::MarkRead { provider_ids })?;
        }
        self.reload()
    }

    pub fn mark_unread(&mut self, thread_id: &str) -> Result<()> {
        let thread = self
            .thread(thread_id)
            .ok_or_else(|| CoreError::NotFound(thread_id.into()))?;
        let last = thread
            .messages
            .last()
            .ok_or_else(|| CoreError::NotFound(thread_id.into()))?;
        let headers: Vec<MessageHeader> = last
            .copies
            .iter()
            .map(|&i| self.cache.headers[i].clone())
            .collect();
        for h in &headers {
            self.store.set_flags(
                h.id,
                Flags {
                    seen: false,
                    ..h.flags
                },
            )?;
        }
        for (account, provider_ids) in self.provider_ids_by_account(headers.iter()) {
            self.queue_action(&account, &RemoteAction::MarkUnread { provider_ids })?;
        }
        self.reload()
    }

    fn update_state(&mut self, thread_id: &str, change: impl Fn(&mut ThreadState)) -> Result<()> {
        let thread = self
            .thread(thread_id)
            .ok_or_else(|| CoreError::NotFound(thread_id.into()))?;
        let accounts: HashSet<String> = self.copies(thread).map(|h| h.account_id.clone()).collect();
        let mut state = self.state(thread_id);
        change(&mut state);
        self.store.set_thread_state(thread_id, state, self.now())?;
        for account in accounts {
            self.queue_action(
                &account,
                &RemoteAction::SetThreadState {
                    thread_id: thread_id.into(),
                    state,
                },
            )?;
        }
        self.reload()
    }

    pub fn toggle_pin(&mut self, conversation_id: &str) -> Result<()> {
        let ids = self.thread_ids_for(conversation_id);
        let pin = !ids.iter().any(|id| self.state(id).pinned);
        for id in ids {
            self.update_state(&id, |s| s.pinned = pin)?;
        }
        Ok(())
    }

    /// Archive a conversation: out of every inbox, into each account's archive.
    pub fn mark_done(&mut self, conversation_id: &str) -> Result<()> {
        self.move_conversation(conversation_id, true)
    }

    /// Bring a Done conversation back to the inbox.
    pub fn undo_done(&mut self, conversation_id: &str) -> Result<()> {
        self.move_conversation(conversation_id, false)
    }

    fn move_conversation(&mut self, conversation_id: &str, done: bool) -> Result<()> {
        let (from, to) = if done {
            (FolderRole::Inbox, FolderRole::Archive)
        } else {
            (FolderRole::Archive, FolderRole::Inbox)
        };
        let mut moved: Vec<MessageHeader> = Vec::new();
        for id in self.thread_ids_for(conversation_id) {
            if let Some(thread) = self.thread(&id) {
                moved.extend(
                    self.copies(thread)
                        .filter(|h| {
                            self.cache
                                .roles
                                .get(&h.id)
                                .is_some_and(|r| r.contains(&from))
                        })
                        .cloned(),
                );
            }
        }
        for h in &moved {
            let target = self.store.folder_by_role(&h.account_id, to)?;
            self.store.move_to_role(h.id, from, target)?;
        }
        for (account, provider_ids) in self.provider_ids_by_account(moved.iter()) {
            let action = if done {
                RemoteAction::Archive { provider_ids }
            } else {
                RemoteAction::MoveToInbox { provider_ids }
            };
            self.queue_action(&account, &action)?;
        }
        for id in self.thread_ids_for(conversation_id) {
            let mut state = self.store.thread_state(&id)?;
            state.done = done;
            self.store.set_thread_state(&id, state, self.now())?;
        }
        self.reload()
    }

    /// Reply to everyone in a conversation. The message waits in the outbox for
    /// the undo-send window before it goes out.
    pub fn send_reply(&mut self, conversation_id: &str, text: &str) -> Result<PendingSend> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CoreError::Build("the message is empty".into()));
        }
        let thread = self
            .resolve_thread(conversation_id)
            .ok_or_else(|| CoreError::NotFound(conversation_id.into()))?;
        let thread_id = thread.id.clone();
        let (account, to, cc, base) = self
            .reply_target(thread)
            .ok_or_else(|| CoreError::NotFound("someone to reply to".into()))?;
        let root_subject = self.cache.headers[thread.messages[0].index].subject.clone();
        let (base_parts, _) = self.message_parts(base);
        let mut references = base.references.clone();
        if let Some(id) = &base.rfc_message_id {
            references.push(id.clone());
        }
        if references.len() > MAX_REFERENCES {
            references.drain(..references.len() - MAX_REFERENCES);
        }
        let now = self.now();
        let domain = account
            .email
            .split('@')
            .nth(1)
            .unwrap_or("lanternpost.local");
        let outgoing = OutgoingMessage {
            message_id: format!("{}@{domain}", uuid::Uuid::new_v4()),
            from: Address::new(Some(&account.display_name), &account.email),
            to: if to.is_empty() {
                vec![Address::new(None, &account.email)]
            } else {
                to
            },
            cc,
            bcc: Vec::new(),
            subject: reply_subject(&root_subject),
            in_reply_to: base.rfc_message_id.clone(),
            references,
            date: now,
            text: text.to_string(),
            quote: Some(QuotedOriginal {
                from: base.from.clone(),
                date_label: self.clock().attribution_label(base.date),
                text: base_parts.new_text,
            }),
        };
        let raw = build_message(&outgoing).map_err(|e| CoreError::Build(e.to_string()))?;
        let raw = String::from_utf8(raw).map_err(|e| CoreError::Build(e.to_string()))?;
        let payload = serde_json::to_string(&SendPayload {
            thread_id: thread_id.clone(),
            raw,
            text: text.to_string(),
            date: now,
        })
        .map_err(|e| CoreError::Build(e.to_string()))?;
        let send_at = now + self.undo_seconds;
        let outbox_id = self
            .store
            .enqueue(&account.id, SEND_KIND, &payload, send_at, now)?;
        Ok(PendingSend {
            outbox_id,
            thread_id,
            send_at,
            seconds_left: self.undo_seconds,
        })
    }

    /// Cancel a send that is still in its undo window. Returns the text so the
    /// UI can put it back in the chat box, or `None` if it already went out.
    pub fn undo_send(&mut self, outbox_id: i64) -> Result<Option<String>> {
        let item = self
            .store
            .pending_outbox()?
            .into_iter()
            .find(|i| i.id == outbox_id && i.kind == SEND_KIND);
        let Some(item) = item else {
            return Ok(None);
        };
        if item.not_before <= self.now() {
            return Ok(None);
        }
        let payload: SendPayload =
            serde_json::from_str(&item.payload).map_err(|e| CoreError::Build(e.to_string()))?;
        self.store.dequeue(outbox_id)?;
        Ok(Some(payload.text))
    }

    pub fn pending_sends(&self) -> Vec<PendingSend> {
        let now = self.now();
        self.store
            .pending_outbox()
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.kind == SEND_KIND && i.attempts == 0)
            .filter_map(|i| {
                let p: SendPayload = serde_json::from_str(&i.payload).ok()?;
                Some(PendingSend {
                    outbox_id: i.id,
                    thread_id: p.thread_id,
                    send_at: i.not_before,
                    seconds_left: (i.not_before - now).max(0),
                })
            })
            .collect()
    }

    /// Send everything that is due. Returns how many items were processed.
    /// The UI calls this on a timer; the sync engine will later call it too.
    pub fn process_outbox(&mut self) -> Result<usize> {
        let now = self.now();
        let due = self.store.due_outbox(now)?;
        let mut done = 0;
        let mut sent_any = false;
        for item in due {
            let Some(account) = self.account(&item.account_id).cloned() else {
                self.store.dequeue(item.id)?;
                continue;
            };
            let result = match item.kind.as_str() {
                SEND_KIND => match serde_json::from_str::<SendPayload>(&item.payload) {
                    Ok(p) => self
                        .transport
                        .send(&account, p.raw.as_bytes())
                        .map(|()| Some(p)),
                    Err(e) => Err(e.to_string()),
                },
                ACTION_KIND => match serde_json::from_str::<RemoteAction>(&item.payload) {
                    Ok(action) => self.transport.apply(&account, &action).map(|()| None),
                    Err(e) => Err(e.to_string()),
                },
                _ => Ok(None),
            };
            match result {
                Ok(sent) => {
                    self.store.dequeue(item.id)?;
                    if let Some(p) = sent {
                        // Keep a local copy in Sent until the provider's copy syncs.
                        let sent_folder =
                            self.store.folder_by_role(&account.id, FolderRole::Sent)?;
                        self.ingest(&Incoming {
                            account_id: account.id.clone(),
                            provider_id: format!("local:{}", item.id),
                            provider_thread_id: None,
                            raw: p.raw.into_bytes(),
                            folder_ids: sent_folder.into_iter().collect(),
                            flags: Flags {
                                seen: true,
                                ..Flags::default()
                            },
                            received_at: now,
                        })?;
                        sent_any = true;
                    }
                    done += 1;
                }
                Err(error) => self.store.outbox_failed(item.id, &error, now)?,
            }
        }
        if sent_any || done > 0 {
            self.reload()?;
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests;
