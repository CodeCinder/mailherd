//! IMAP and SMTP mail, used for iCloud.
//!
//! - Inbox, Sent and Archive are synced. The first pass downloads the last
//!   [`INITIAL_SYNC_DAYS`] days (up to [`INITIAL_SYNC_LIMIT`] per folder).
//! - Each later pass downloads new UIDs, refreshes flags of known mail and
//!   notices deletions.
//! - A message's provider id is `uidvalidity:uid:folder`, so the folder name
//!   (which may contain any character) comes last.

use std::collections::{BTreeMap, BTreeSet};

use imap::types::{Fetch, Flag};
use imap::{ClientBuilder, ConnectionMode, TlsKind};
use imap_proto::NameAttribute;
use lettre::Transport;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use mh_app::RemoteAction;
use mh_app::sync::{
    INITIAL_SYNC_DAYS, INITIAL_SYNC_LIMIT, MailProvider, ProviderError, RemoteFolder, SyncEvent,
    SyncStatus,
};
use mh_model::{AccountId, Flags};
use mh_store::FolderRole;
use serde::{Deserialize, Serialize};

type Session = imap::Session<imap::Connection>;

const FETCH_BATCH: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImapConfig {
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// False only for tests against a local server.
    pub tls: bool,
    pub username: String,
    pub password: String,
}

impl ImapConfig {
    /// iCloud Mail. `email` is the @icloud.com address; `password` is an
    /// app-specific password from account.apple.com.
    pub fn icloud(email: &str, password: &str) -> Self {
        Self {
            imap_host: "imap.mail.me.com".into(),
            imap_port: 993,
            smtp_host: "smtp.mail.me.com".into(),
            smtp_port: 587,
            tls: true,
            username: email.trim().to_string(),
            password: password.trim().to_string(),
        }
    }
}

pub struct Imap {
    account_id: AccountId,
    config: ImapConfig,
    session: Option<Session>,
    folders: Vec<RemoteFolder>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FolderCursor {
    uidvalidity: u32,
    uids: BTreeSet<u32>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cursor {
    #[serde(default)]
    folders: BTreeMap<String, FolderCursor>,
}

fn imap_error(e: imap::Error) -> ProviderError {
    match e {
        imap::Error::Io(_) | imap::Error::ConnectionLost => ProviderError::Retry(e.to_string()),
        imap::Error::No(ref r) if r.information.to_ascii_uppercase().contains("AUTH") => {
            ProviderError::NeedsSignIn
        }
        other => ProviderError::Failed(format!("IMAP: {other}")),
    }
}

/// `1:5,7,9:12` from a set of UIDs.
fn uid_set(uids: &BTreeSet<u32>) -> String {
    let mut parts = Vec::new();
    let mut iter = uids.iter().copied().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end + 1)) {
            end += 1;
            iter.next();
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
    }
    parts.join(",")
}

fn imap_date(ts: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let dt =
        time::OffsetDateTime::from_unix_timestamp(ts).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{}-{}-{}",
        dt.day(),
        MONTHS[usize::from(u8::from(dt.month())) - 1],
        dt.year()
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

fn flags_of(fetch: &Fetch<'_>) -> Flags {
    let f = fetch.flags();
    Flags {
        seen: f.contains(&Flag::Seen),
        flagged: f.contains(&Flag::Flagged),
        answered: f.contains(&Flag::Answered),
        draft: f.contains(&Flag::Draft),
    }
}

pub fn provider_id(uidvalidity: u32, uid: u32, folder: &str) -> String {
    format!("{uidvalidity}:{uid}:{folder}")
}

/// (uidvalidity, uid, folder) from a provider id.
fn parse_provider_id(id: &str) -> Option<(u32, u32, &str)> {
    let mut parts = id.splitn(3, ':');
    let validity = parts.next()?.parse().ok()?;
    let uid = parts.next()?.parse().ok()?;
    Some((validity, uid, parts.next()?))
}

fn role_for(name: &str, attributes: &[NameAttribute<'_>]) -> Option<FolderRole> {
    for a in attributes {
        match a {
            NameAttribute::Sent => return Some(FolderRole::Sent),
            NameAttribute::Archive => return Some(FolderRole::Archive),
            NameAttribute::Drafts => return Some(FolderRole::Drafts),
            NameAttribute::Trash => return Some(FolderRole::Trash),
            NameAttribute::Junk => return Some(FolderRole::Spam),
            _ => {}
        }
    }
    // Servers without special-use attributes: go by iCloud's and common names.
    match name.to_ascii_lowercase().as_str() {
        "inbox" => Some(FolderRole::Inbox),
        "sent messages" | "sent" | "sent items" => Some(FolderRole::Sent),
        "archive" => Some(FolderRole::Archive),
        "drafts" => Some(FolderRole::Drafts),
        "deleted messages" | "trash" => Some(FolderRole::Trash),
        "junk" | "spam" => Some(FolderRole::Spam),
        _ => None,
    }
}

impl Imap {
    pub fn new(account_id: AccountId, config: ImapConfig) -> Self {
        Self {
            account_id,
            config,
            session: None,
            folders: Vec::new(),
        }
    }

    fn connect(config: &ImapConfig) -> Result<Session, ProviderError> {
        let mode = if config.tls {
            ConnectionMode::AutoTls
        } else {
            ConnectionMode::Plaintext
        };
        let client = ClientBuilder::new(config.imap_host.as_str(), config.imap_port)
            .mode(mode)
            .tls_kind(TlsKind::Rust)
            .connect()
            .map_err(|e| ProviderError::Retry(format!("can't reach {}: {e}", config.imap_host)))?;
        client
            .login(&config.username, &config.password)
            .map_err(|(e, _)| match e {
                imap::Error::No(_) | imap::Error::Bad(_) => ProviderError::NeedsSignIn,
                other => imap_error(other),
            })
    }

    /// Check an address and password work, for the sign-in screen.
    pub fn check_login(config: &ImapConfig) -> Result<(), ProviderError> {
        let mut session = Self::connect(config)?;
        let _ = session.logout();
        Ok(())
    }

    fn session(&mut self) -> Result<&mut Session, ProviderError> {
        if self.session.is_none() {
            self.session = Some(Self::connect(&self.config)?);
        }
        self.session
            .as_mut()
            .ok_or_else(|| ProviderError::Retry("not connected".into()))
    }

    /// Run an IMAP step; a dropped connection is reopened on the next call.
    fn with_session<T>(
        &mut self,
        f: impl FnOnce(&mut Session) -> imap::Result<T>,
    ) -> Result<T, ProviderError> {
        let result = f(self.session()?);
        if let Err(imap::Error::Io(_) | imap::Error::ConnectionLost) = &result {
            self.session = None;
        }
        result.map_err(imap_error)
    }

    fn list_folders(&mut self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let names = self.with_session(|s| s.list(Some(""), Some("*")))?;
        let folders: Vec<RemoteFolder> = names
            .iter()
            .filter(|n| !n.attributes().contains(&NameAttribute::NoSelect))
            .map(|n| RemoteFolder {
                provider_id: n.name().to_string(),
                name: if n.name().eq_ignore_ascii_case("INBOX") {
                    "Inbox".into()
                } else {
                    n.name().to_string()
                },
                role: role_for(n.name(), n.attributes()),
            })
            .collect();
        self.folders = folders.clone();
        Ok(folders)
    }

    fn folder_with(&self, role: FolderRole) -> Option<String> {
        self.folders
            .iter()
            .find(|f| f.role == Some(role))
            .map(|f| f.provider_id.clone())
    }

    fn sync_folder(
        &mut self,
        folder: &str,
        previous: Option<FolderCursor>,
        first_sync: bool,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> Result<FolderCursor, ProviderError> {
        let mailbox = self.with_session(|s| s.select(folder))?;
        let uidvalidity = mailbox.uid_validity.unwrap_or(0);
        // A changed UIDVALIDITY means every UID in the folder was renumbered.
        let known = match previous {
            Some(p) if p.uidvalidity == uidvalidity => p.uids,
            Some(p) => {
                for uid in p.uids {
                    emit(SyncEvent::Removed {
                        account_id: self.account_id.clone(),
                        provider_id: provider_id(p.uidvalidity, uid, folder),
                    });
                }
                BTreeSet::new()
            }
            None => BTreeSet::new(),
        };

        // Which known messages still exist, and their current flags.
        let mut still_there = BTreeSet::new();
        if !known.is_empty() {
            let set = uid_set(&known);
            let fetched = self.with_session(|s| s.uid_fetch(&set, "(UID FLAGS)"))?;
            for f in fetched.iter() {
                let Some(uid) = f.uid else { continue };
                if f.flags().contains(&Flag::Deleted) {
                    continue;
                }
                still_there.insert(uid);
                emit(SyncEvent::State {
                    account_id: self.account_id.clone(),
                    provider_id: provider_id(uidvalidity, uid, folder),
                    flags: Some(flags_of(f)),
                    folders: None,
                });
            }
            for uid in known.difference(&still_there) {
                emit(SyncEvent::Removed {
                    account_id: self.account_id.clone(),
                    provider_id: provider_id(uidvalidity, *uid, folder),
                });
            }
        }

        // New mail: everything above the highest known UID, or the recent
        // window on the first pass.
        let query = match known.iter().next_back() {
            Some(max) => format!("UID {}:*", max + 1),
            None => format!("SINCE {}", imap_date(now() - INITIAL_SYNC_DAYS * 86_400)),
        };
        let found = self.with_session(|s| s.uid_search(&query))?;
        // `UID n:*` always matches the last message, even when it's below n.
        let floor = known.iter().next_back().copied().unwrap_or(0);
        let mut new: Vec<u32> = found.into_iter().filter(|u| *u > floor).collect();
        new.sort_unstable_by(|a, b| b.cmp(a));
        new.truncate(INITIAL_SYNC_LIMIT);

        let total = new.len();
        for (i, chunk) in new.chunks(FETCH_BATCH).enumerate() {
            if first_sync && total > FETCH_BATCH {
                emit(SyncEvent::Status {
                    account_id: self.account_id.clone(),
                    status: SyncStatus::Syncing {
                        done: i * FETCH_BATCH,
                        total: Some(total),
                    },
                });
            }
            let set = uid_set(&chunk.iter().copied().collect());
            let fetched =
                self.with_session(|s| s.uid_fetch(&set, "(UID FLAGS INTERNALDATE BODY.PEEK[])"))?;
            for f in fetched.iter() {
                let (Some(uid), Some(body)) = (f.uid, f.body()) else {
                    continue;
                };
                if f.flags().contains(&Flag::Deleted) {
                    continue;
                }
                still_there.insert(uid);
                emit(SyncEvent::Message {
                    account_id: self.account_id.clone(),
                    provider_id: provider_id(uidvalidity, uid, folder),
                    provider_thread_id: None,
                    raw: body.to_vec(),
                    folders: vec![folder.to_string()],
                    flags: flags_of(f),
                    received_at: f.internal_date().map_or(0, |d| d.timestamp()),
                });
            }
        }
        Ok(FolderCursor {
            uidvalidity,
            uids: still_there,
        })
    }

    /// Group provider ids by folder: folder -> UID set.
    fn by_folder(ids: &[String]) -> BTreeMap<String, BTreeSet<u32>> {
        let mut map: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for id in ids {
            if let Some((_, uid, folder)) = parse_provider_id(id) {
                map.entry(folder.to_string()).or_default().insert(uid);
            }
        }
        map
    }

    fn store_flags(&mut self, ids: &[String], query: &str) -> Result<(), ProviderError> {
        for (folder, uids) in Self::by_folder(ids) {
            let set = uid_set(&uids);
            self.with_session(|s| {
                s.select(&folder)?;
                s.uid_store(&set, query).map(|_| ())
            })?;
        }
        Ok(())
    }

    fn move_to(&mut self, ids: &[String], target: Option<String>) -> Result<(), ProviderError> {
        let Some(target) = target else {
            return Err(ProviderError::Failed(
                "this account has no folder to move to".into(),
            ));
        };
        for (folder, uids) in Self::by_folder(ids) {
            if folder == target {
                continue;
            }
            let set = uid_set(&uids);
            self.with_session(|s| {
                s.select(&folder)?;
                s.uid_mv(&set, &target)
            })?;
        }
        Ok(())
    }
}

impl MailProvider for Imap {
    fn sync(
        &mut self,
        cursor: Option<&str>,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> Result<String, ProviderError> {
        emit(SyncEvent::Status {
            account_id: self.account_id.clone(),
            status: SyncStatus::Connecting,
        });
        let mut state: Cursor = cursor
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        let folders = self.list_folders()?;
        emit(SyncEvent::Folders {
            account_id: self.account_id.clone(),
            folders: folders.clone(),
        });

        let synced: Vec<String> = [FolderRole::Inbox, FolderRole::Sent, FolderRole::Archive]
            .into_iter()
            .filter_map(|role| self.folder_with(role))
            .collect();
        let mut next = BTreeMap::new();
        for folder in synced {
            let previous = state.folders.remove(&folder);
            let first = previous.is_none();
            let result = self.sync_folder(&folder, previous, first, emit)?;
            next.insert(folder, result);
        }
        emit(SyncEvent::Status {
            account_id: self.account_id.clone(),
            status: SyncStatus::UpToDate { at: now() },
        });
        serde_json::to_string(&Cursor { folders: next })
            .map_err(|e| ProviderError::Failed(e.to_string()))
    }

    fn send(&mut self, raw: &[u8]) -> Result<(), ProviderError> {
        let parsed =
            mh_mime::parse_message(raw).map_err(|e| ProviderError::Failed(e.to_string()))?;
        let from = parsed
            .from
            .as_ref()
            .and_then(|a| a.email.parse().ok())
            .ok_or_else(|| ProviderError::Failed("the message has no valid From address".into()))?;
        let recipients = parsed
            .to
            .iter()
            .chain(parsed.cc.iter())
            .filter_map(|a| a.email.parse().ok())
            .collect();
        let envelope = Envelope::new(Some(from), recipients)
            .map_err(|e| ProviderError::Failed(e.to_string()))?;
        let credentials =
            Credentials::new(self.config.username.clone(), self.config.password.clone());
        let transport = if self.config.tls {
            lettre::SmtpTransport::starttls_relay(&self.config.smtp_host)
                .map_err(|e| ProviderError::Failed(e.to_string()))?
                .port(self.config.smtp_port)
                .credentials(credentials)
                .build()
        } else {
            lettre::SmtpTransport::builder_dangerous(self.config.smtp_host.as_str())
                .port(self.config.smtp_port)
                .build()
        };
        transport.send_raw(&envelope, raw).map_err(|e| {
            if e.is_permanent() {
                ProviderError::Failed(format!("the mail server refused the message: {e}"))
            } else {
                ProviderError::Retry(e.to_string())
            }
        })?;

        // iCloud doesn't file sent mail by itself, so put a copy in Sent.
        if self.folders.is_empty() {
            self.list_folders()?;
        }
        if let Some(sent) = self.folder_with(FolderRole::Sent) {
            self.with_session(|s| s.append(&sent, raw).flag(Flag::Seen).finish().map(|_| ()))?;
        }
        Ok(())
    }

    fn apply(&mut self, action: &RemoteAction) -> Result<(), ProviderError> {
        if self.folders.is_empty() {
            self.list_folders()?;
        }
        match action {
            RemoteAction::MarkRead { provider_ids } => {
                self.store_flags(provider_ids, "+FLAGS.SILENT (\\Seen)")
            }
            RemoteAction::MarkUnread { provider_ids } => {
                self.store_flags(provider_ids, "-FLAGS.SILENT (\\Seen)")
            }
            RemoteAction::Archive { provider_ids } => {
                let target = self.folder_with(FolderRole::Archive);
                self.move_to(provider_ids, target)
            }
            RemoteAction::MoveToInbox { provider_ids } => {
                let target = self.folder_with(FolderRole::Inbox);
                self.move_to(provider_ids, target)
            }
            RemoteAction::SetThreadState { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
