//! Shared data types used across the MailHerd core.
//!
//! These types are deliberately plain (strings, integers, vectors) so they
//! cross the FFI boundary to each native UI without surprises.

use serde::{Deserialize, Serialize};

/// Stable identifier of an account, generated when the account is added.
pub type AccountId = String;

/// Local identifier of a stored message copy (one per account that holds it).
pub type MessageId = i64;

/// Seconds since the Unix epoch, UTC.
pub type Timestamp = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderKind {
    ICloud,
    Gmail,
    Microsoft365,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,
    pub kind: ProviderKind,
    pub email: String,
    pub display_name: String,
    /// Accent colour shown next to this account's mail, as 0xRRGGBB.
    pub color: u32,
}

/// An email address with an optional display name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

impl Address {
    pub fn new(name: Option<&str>, email: &str) -> Self {
        let name = name
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(String::from);
        Self {
            name,
            email: email.trim().to_string(),
        }
    }

    /// Lowercased address used for comparisons and grouping.
    pub fn normalized(&self) -> String {
        normalize_email(&self.email)
    }

    /// The name to show for this address: display name, else the local part.
    pub fn display(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => self
                .email
                .split('@')
                .next()
                .unwrap_or(&self.email)
                .to_string(),
        }
    }
}

/// Lowercase an email address and trim whitespace.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Strip the angle brackets and whitespace around a Message-ID.
pub fn normalize_message_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_string()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub filename: Option<String>,
    pub mime_type: String,
    pub size: usize,
    pub content_id: Option<String>,
    /// True when the part is meant to be shown inline (for example an image in the body).
    pub inline: bool,
}

/// A message body after parsing. HTML is always sanitised before it is stored here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Body {
    pub text: Option<String>,
    pub html: Option<String>,
}

/// Header-level information about one stored copy of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHeader {
    pub id: MessageId,
    pub account_id: AccountId,
    /// The provider's own identifier: IMAP UID, Gmail message id or Graph id.
    pub provider_id: String,
    /// Native thread id from Gmail (`threadId`) or Graph (`conversationId`).
    pub provider_thread_id: Option<String>,
    /// RFC 5322 Message-ID without angle brackets.
    pub rfc_message_id: Option<String>,
    pub in_reply_to: Vec<String>,
    pub references: Vec<String>,
    pub subject: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub reply_to: Vec<Address>,
    pub date: Timestamp,
    pub flags: Flags,
    pub has_attachments: bool,
    /// Short preview of the new text in this message.
    pub snippet: String,
}

/// Category assigned by the smart-category classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    People,
    Newsletters,
    Notifications,
    Receipts,
}

/// App-only state that is mirrored into the mailbox so every device sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadState {
    pub pinned: bool,
    pub done: bool,
    pub snoozed_until: Option<Timestamp>,
    pub remind_if_no_reply_at: Option<Timestamp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_display_prefers_name_then_local_part() {
        assert_eq!(Address::new(Some(" Ada "), "ada@x.org").display(), "Ada");
        assert_eq!(Address::new(None, "grace@x.org").display(), "grace");
        assert_eq!(Address::new(Some(""), "grace@x.org").display(), "grace");
    }

    #[test]
    fn message_ids_are_normalized() {
        assert_eq!(normalize_message_id(" <abc@host> "), "abc@host");
        assert_eq!(normalize_message_id("abc@host"), "abc@host");
    }
}
