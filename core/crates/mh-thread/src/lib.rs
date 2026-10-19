//! Grouping messages into conversations.
//!
//! Messages are joined into one thread when any of these hold:
//! - they share a provider thread id (Gmail `threadId`, Graph `conversationId`)
//!   in the same account;
//! - one references the other's Message-ID, or both reference the same
//!   Message-ID (even one we never downloaded);
//! - they are copies of the same message in different accounts;
//! - as a last resort, a reply with no threading headers matches the subject
//!   and a participant of an earlier message from the previous 60 days.
//!
//! The same message held by two accounts appears once in its thread.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use mh_model::{AccountId, Address, MessageHeader, Timestamp};
use regex::Regex;

const SUBJECT_WINDOW_SECS: Timestamp = 60 * 24 * 60 * 60;

/// One conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    /// Stable id derived from the earliest message in the thread.
    pub id: String,
    /// Messages oldest first, with copies of the same message merged.
    pub messages: Vec<ThreadMessage>,
}

/// A message as it appears in a thread: one entry even if several accounts hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadMessage {
    /// Index into the slice passed to [`build_threads`] for the copy shown.
    pub index: usize,
    /// Every account that holds a copy, in input order.
    pub accounts: Vec<AccountId>,
    /// Indexes of every copy, including `index`.
    pub copies: Vec<usize>,
}

impl Thread {
    pub fn last_index(&self) -> Option<usize> {
        self.messages.last().map(|m| m.index)
    }
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new() -> Self {
        Self { parent: Vec::new() }
    }

    fn add(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.parent.len() - 1
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[rb] = ra;
        }
    }
}

static REPLY_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*((re|fwd?|aw|wg|sv|vs|antw|tr|rif|enc|odp|pd|ynt|回复|答复|转发|転送)\s*(\[\d+\]|\(\d+\))?\s*[:：]\s*)+")
        .expect("valid regex")
});

/// Subject without reply/forward prefixes, lowercased, whitespace collapsed.
pub fn normalize_subject(subject: &str) -> String {
    let stripped = REPLY_PREFIX.replace(subject, "");
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// True when the subject starts with a reply prefix such as "Re:".
pub fn is_reply_subject(subject: &str) -> bool {
    REPLY_PREFIX.is_match(subject)
}

fn participants(m: &MessageHeader) -> impl Iterator<Item = String> + '_ {
    std::iter::once(&m.from)
        .chain(m.to.iter())
        .chain(m.cc.iter())
        .map(Address::normalized)
}

/// Group messages into threads, newest thread first.
pub fn build_threads(messages: &[MessageHeader]) -> Vec<Thread> {
    let mut uf = UnionFind::new();
    for _ in messages {
        uf.add();
    }
    let mut id_nodes: HashMap<String, usize> = HashMap::new();
    let mut node_for = |uf: &mut UnionFind, key: String| -> usize {
        *id_nodes.entry(key).or_insert_with(|| uf.add())
    };

    for (i, m) in messages.iter().enumerate() {
        if let Some(id) = &m.rfc_message_id {
            let node = node_for(&mut uf, format!("mid:{id}"));
            uf.union(i, node);
        }
        for r in m.references.iter().chain(m.in_reply_to.iter()) {
            let node = node_for(&mut uf, format!("mid:{r}"));
            uf.union(i, node);
        }
        if let Some(t) = &m.provider_thread_id {
            let node = node_for(&mut uf, format!("pt:{}:{t}", m.account_id));
            uf.union(i, node);
        }
    }

    // Subject fallback for replies that carry no threading information at all.
    let mut by_date: Vec<usize> = (0..messages.len()).collect();
    by_date.sort_by_key(|&i| messages[i].date);
    for (pos, &i) in by_date.iter().enumerate() {
        let m = &messages[i];
        let unlinked =
            m.references.is_empty() && m.in_reply_to.is_empty() && m.provider_thread_id.is_none();
        if !unlinked || !is_reply_subject(&m.subject) {
            continue;
        }
        let subject = normalize_subject(&m.subject);
        if subject.is_empty() {
            continue;
        }
        let people: HashSet<String> = participants(m).collect();
        let earlier = by_date[..pos].iter().rev().copied().find(|&j| {
            let o = &messages[j];
            m.date - o.date <= SUBJECT_WINDOW_SECS
                && normalize_subject(&o.subject) == subject
                && participants(o).any(|p| people.contains(&p))
        });
        if let Some(j) = earlier {
            uf.union(i, j);
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..messages.len() {
        groups.entry(uf.find(i)).or_default().push(i);
    }

    let mut threads: Vec<Thread> = groups
        .into_values()
        .map(|mut members| {
            members.sort_by_key(|&i| (messages[i].date, i));
            let mut merged: Vec<ThreadMessage> = Vec::new();
            let mut seen: HashMap<String, usize> = HashMap::new();
            for i in members {
                let m = &messages[i];
                if let Some(id) = &m.rfc_message_id
                    && let Some(&slot) = seen.get(id)
                {
                    let entry = &mut merged[slot];
                    if !entry.accounts.contains(&m.account_id) {
                        entry.accounts.push(m.account_id.clone());
                    }
                    entry.copies.push(i);
                    continue;
                }
                if let Some(id) = &m.rfc_message_id {
                    seen.insert(id.clone(), merged.len());
                }
                merged.push(ThreadMessage {
                    index: i,
                    accounts: vec![m.account_id.clone()],
                    copies: vec![i],
                });
            }
            let root = &messages[merged[0].index];
            let id = match &root.rfc_message_id {
                Some(mid) => format!("t:{mid}"),
                None => format!("t:{}:{}", root.account_id, root.provider_id),
            };
            Thread {
                id,
                messages: merged,
            }
        })
        .collect();

    threads.sort_by(|a, b| {
        let last = |t: &Thread| t.last_index().map_or(0, |i| messages[i].date);
        last(b).cmp(&last(a)).then_with(|| a.id.cmp(&b.id))
    });
    threads
}

/// Key for the People view: everyone in the thread except you, sorted.
/// A one-to-one conversation has one entry; a group has several.
pub fn people_key<'a>(
    addresses: impl IntoIterator<Item = &'a Address>,
    my_addresses: &HashSet<String>,
) -> Vec<String> {
    let set: BTreeSet<String> = addresses
        .into_iter()
        .map(Address::normalized)
        .filter(|a| !a.is_empty() && !my_addresses.contains(a))
        .collect();
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mh_model::Flags;

    fn msg(
        account: &str,
        id: Option<&str>,
        refs: &[&str],
        subject: &str,
        date: Timestamp,
    ) -> MessageHeader {
        MessageHeader {
            id: 0,
            account_id: account.to_string(),
            provider_id: format!("{account}-{date}"),
            provider_thread_id: None,
            rfc_message_id: id.map(String::from),
            in_reply_to: refs.last().map(|r| vec![r.to_string()]).unwrap_or_default(),
            references: refs.iter().map(|r| r.to_string()).collect(),
            subject: subject.to_string(),
            from: Address::new(Some("Ada"), "ada@example.org"),
            to: vec![Address::new(None, "me@example.org")],
            cc: vec![],
            reply_to: vec![],
            date,
            flags: Flags::default(),
            has_attachments: false,
            snippet: String::new(),
        }
    }

    #[test]
    fn replies_join_their_parents() {
        let msgs = vec![
            msg("a", Some("1@x"), &[], "Lunch", 100),
            msg("a", Some("2@x"), &["1@x"], "Re: Lunch", 200),
            msg("a", Some("3@x"), &["1@x", "2@x"], "Re: Lunch", 300),
            msg("a", Some("9@x"), &[], "Other", 250),
        ];
        let threads = build_threads(&msgs);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "t:1@x");
        let order: Vec<usize> = threads[0].messages.iter().map(|m| m.index).collect();
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn siblings_of_a_missing_parent_share_a_thread() {
        let msgs = vec![
            msg("a", Some("2@x"), &["root@x"], "Re: Plan", 200),
            msg("a", Some("3@x"), &["root@x"], "Re: Plan", 300),
        ];
        assert_eq!(build_threads(&msgs).len(), 1);
    }

    #[test]
    fn copies_in_two_accounts_merge_into_one_message() {
        let msgs = vec![
            msg("icloud", Some("1@x"), &[], "Trip", 100),
            msg("gmail", Some("1@x"), &[], "Trip", 100),
            msg("gmail", Some("2@x"), &["1@x"], "Re: Trip", 200),
        ];
        let threads = build_threads(&msgs);
        assert_eq!(threads.len(), 1);
        let t = &threads[0];
        assert_eq!(t.messages.len(), 2);
        assert_eq!(t.messages[0].accounts, vec!["icloud", "gmail"]);
        assert_eq!(t.messages[0].copies, vec![0, 1]);
    }

    #[test]
    fn provider_thread_ids_group_within_one_account_only() {
        let mut a = msg("gmail", Some("1@x"), &[], "Hi", 100);
        let mut b = msg("gmail", Some("2@x"), &[], "Hi again", 200);
        let mut c = msg("graph", Some("3@x"), &[], "Unrelated", 300);
        a.provider_thread_id = Some("T1".into());
        b.provider_thread_id = Some("T1".into());
        c.provider_thread_id = Some("T1".into());
        let threads = build_threads(&[a, b, c]);
        assert_eq!(threads.len(), 2);
    }

    #[test]
    fn subject_fallback_needs_a_reply_prefix_and_shared_people() {
        let original = msg("a", Some("1@x"), &[], "Budget", 100);
        let reply = msg("a", Some("2@x"), &[], "RE: Budget", 200);
        let mut stranger = msg("a", Some("3@x"), &[], "Re: Budget", 300);
        stranger.from = Address::new(None, "eve@else.org");
        stranger.to = vec![Address::new(None, "zed@else.org")];
        let new_topic = msg("a", Some("4@x"), &[], "Budget", 400);
        let threads = build_threads(&[original, reply, stranger, new_topic]);
        let sizes: Vec<usize> = threads.iter().map(|t| t.messages.len()).collect();
        assert_eq!(threads.len(), 3, "sizes: {sizes:?}");
        assert!(sizes.contains(&2));
    }

    #[test]
    fn subjects_are_normalized() {
        assert_eq!(
            normalize_subject("Re: Fwd: RE[2]:  Budget  Q4"),
            "budget q4"
        );
        assert_eq!(normalize_subject("AW: WG: Angebot"), "angebot");
        assert_eq!(normalize_subject("回复：会议"), "会议");
        assert!(is_reply_subject("Re: x"));
        assert!(!is_reply_subject("Regarding x"));
    }

    #[test]
    fn people_key_excludes_me_and_sorts() {
        let me: HashSet<String> = ["me@example.org".to_string()].into();
        let people = [
            Address::new(None, "Zed@Example.org"),
            Address::new(None, "me@example.org"),
            Address::new(None, "ada@example.org"),
            Address::new(None, "zed@example.org"),
        ];
        assert_eq!(
            people_key(people.iter(), &me),
            vec!["ada@example.org", "zed@example.org"]
        );
    }
}
