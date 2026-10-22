//! View models: ready-to-render data for every UI.
//!
//! These are plain data and serialise to JSON, which is how they cross into
//! UIs whose bindings prefer it (the Qt app reads them as JavaScript objects).

use serde::Serialize;

/// Which list the user is looking at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "account", rename_all = "snake_case")]
pub enum Mailbox {
    /// Every account's inbox, combined.
    Inbox,
    /// One account's inbox.
    AccountInbox(String),
    Pinned,
    /// Threads marked Done (archived).
    Done,
    /// Threads you have written in.
    Sent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountRow {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub provider: String,
    pub color: String,
    pub unread_count: usize,
    /// "Syncing 120 of 500", "Up to date", "Sign in again"…
    pub status: String,
    pub needs_sign_in: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SidebarItem {
    /// JSON form of the [`Mailbox`], passed back to the core when selected.
    pub mailbox: Mailbox,
    pub label: String,
    pub icon: String,
    pub unread_count: usize,
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sidebar {
    pub items: Vec<SidebarItem>,
    pub accounts: Vec<AccountRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Avatar {
    pub initials: String,
    pub color: String,
}

/// One row in the Threads list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadRow {
    pub thread_id: String,
    pub subject: String,
    /// "Ada, Grace and 2 others".
    pub participants: String,
    pub snippet: String,
    pub date_label: String,
    pub timestamp: i64,
    pub unread: bool,
    pub message_count: usize,
    pub pinned: bool,
    pub has_attachments: bool,
    /// Colours of the accounts that hold this thread.
    pub account_colors: Vec<String>,
    pub avatar: Avatar,
    pub is_bulk: bool,
}

/// One row in the People list: a person, or a group you talk to together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PersonRow {
    /// Sorted, comma-joined addresses; pass back to open the conversation.
    pub key: String,
    pub name: String,
    pub is_group: bool,
    pub snippet: String,
    pub date_label: String,
    pub timestamp: i64,
    pub unread: bool,
    pub thread_count: usize,
    pub pinned: bool,
    pub avatar: Avatar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttachmentRow {
    pub filename: String,
    pub mime_type: String,
    pub size_label: String,
    pub is_image: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Received,
    Sent,
    /// Waiting in the undo-send window.
    Sending,
    Failed,
}

/// One message as a chat bubble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bubble {
    /// Stored message id, or the outbox id (negative) for a message still sending.
    pub message_id: i64,
    pub sender_name: String,
    pub sender_email: String,
    pub is_me: bool,
    /// Only what this message adds; quoted history and signature are split off.
    pub text: String,
    pub quoted: Option<String>,
    pub signature: Option<String>,
    pub forwarded: bool,
    pub has_hidden: bool,
    pub time_label: String,
    pub timestamp: i64,
    pub avatar: Avatar,
    pub attachments: Vec<AttachmentRow>,
    /// Newsletters and other bulk mail render as an expandable card, not a bubble.
    pub is_card: bool,
    pub has_html: bool,
    pub account_colors: Vec<String>,
    pub delivery: DeliveryState,
    /// Shown when the subject changes within a People conversation.
    pub subject_change: Option<String>,
    /// Show the sender's name above the bubble (group chats, first in a run).
    pub show_sender: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationItem {
    Day { label: String },
    Message(Box<Bubble>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conversation {
    /// Thread id, or `people:<key>` for a People conversation.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub is_group: bool,
    pub pinned: bool,
    pub items: Vec<ConversationItem>,
    /// "To Ada, Grace" shown above the chat box.
    pub reply_label: String,
}

/// A send waiting in its undo window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingSend {
    pub outbox_id: i64,
    pub thread_id: String,
    pub send_at: i64,
    pub seconds_left: i64,
}
