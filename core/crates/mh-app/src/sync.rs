//! Changes reported by the sync engine, applied to the store on the UI thread.
//!
//! The sync engine talks to providers on background threads and never touches
//! the store itself. It sends [`SyncEvent`]s; the UI thread passes them to
//! [`Core::apply_sync`], which updates the store and the views in one go.

use mh_model::{Account, AccountId, Flags, Timestamp};
use mh_store::FolderRole;
use serde::Serialize;

use crate::{Core, Incoming, Result};

/// A folder or Gmail label as the provider names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    pub provider_id: String,
    pub name: String,
    pub role: Option<FolderRole>,
}

/// How an account's sync is going, shown next to the account in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SyncStatus {
    Connecting,
    Syncing {
        done: usize,
        total: Option<usize>,
    },
    UpToDate {
        at: Timestamp,
    },
    /// The provider rejected the saved sign-in; the user must sign in again.
    NeedsSignIn,
    /// Temporary trouble (offline, server error); the engine keeps retrying.
    Error {
        message: String,
    },
}

impl SyncStatus {
    pub fn label(&self) -> String {
        match self {
            SyncStatus::Connecting => "Connecting\u{2026}".into(),
            SyncStatus::Syncing {
                done,
                total: Some(total),
            } => {
                format!("Syncing {done} of {total}")
            }
            SyncStatus::Syncing { done, total: None } => format!("Syncing ({done})"),
            SyncStatus::UpToDate { .. } => "Up to date".into(),
            SyncStatus::NeedsSignIn => "Sign in again".into(),
            SyncStatus::Error { message } => format!("Can't connect: {message}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    /// A newly signed-in account.
    AccountAdded { account: Account },
    Folders {
        account_id: AccountId,
        folders: Vec<RemoteFolder>,
    },
    /// A message to store (new, or refetched).
    Message {
        account_id: AccountId,
        provider_id: String,
        provider_thread_id: Option<String>,
        raw: Vec<u8>,
        /// Provider ids of the folders or labels the message is in.
        folders: Vec<String>,
        flags: Flags,
        received_at: Timestamp,
    },
    /// A message's flags or folders changed on the server.
    State {
        account_id: AccountId,
        provider_id: String,
        flags: Option<Flags>,
        folders: Option<Vec<String>>,
    },
    Removed {
        account_id: AccountId,
        provider_id: String,
    },
    /// Where the next incremental sync should start (history id, delta link, UID).
    Cursor {
        account_id: AccountId,
        cursor: String,
    },
    Status {
        account_id: AccountId,
        status: SyncStatus,
    },
}

impl Core {
    fn local_folders(&self, account_id: &str, provider_ids: &[String]) -> Result<Vec<i64>> {
        let mut ids = Vec::new();
        for p in provider_ids {
            if let Some(id) = self.store.folder_id(account_id, p)? {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    /// Apply a batch of sync events. Returns true if the views changed.
    pub fn apply_sync(&mut self, events: Vec<SyncEvent>) -> Result<bool> {
        let mut changed = false;
        for event in events {
            match event {
                SyncEvent::AccountAdded { account } => {
                    self.store.upsert_account(&account)?;
                    changed = true;
                }
                SyncEvent::Folders {
                    account_id,
                    folders,
                } => {
                    for f in folders {
                        self.store
                            .upsert_folder(&account_id, &f.provider_id, &f.name, f.role)?;
                    }
                    changed = true;
                }
                SyncEvent::Message {
                    account_id,
                    provider_id,
                    provider_thread_id,
                    raw,
                    folders,
                    flags,
                    received_at,
                } => {
                    let folder_ids = self.local_folders(&account_id, &folders)?;
                    let incoming = Incoming {
                        account_id: account_id.clone(),
                        provider_id,
                        provider_thread_id,
                        raw,
                        folder_ids,
                        flags,
                        received_at,
                    };
                    // One unreadable message must not stop the rest of the sync.
                    if let Ok(id) = self.ingest(&incoming)
                        && let Some(rfc_id) = self.store.header(id)?.and_then(|h| h.rfc_message_id)
                    {
                        self.store.delete_local_copies(&account_id, &rfc_id)?;
                    }
                    changed = true;
                }
                SyncEvent::State {
                    account_id,
                    provider_id,
                    flags,
                    folders,
                } => {
                    let Some(id) = self.store.message_id(&account_id, &provider_id)? else {
                        continue;
                    };
                    if let Some(flags) = flags {
                        self.store.set_flags(id, flags)?;
                    }
                    if let Some(folders) = folders {
                        let folder_ids = self.local_folders(&account_id, &folders)?;
                        self.store.set_message_folders(id, &folder_ids)?;
                    }
                    changed = true;
                }
                SyncEvent::Removed {
                    account_id,
                    provider_id,
                } => {
                    if let Some(id) = self.store.message_id(&account_id, &provider_id)? {
                        self.store.delete_message(id)?;
                        changed = true;
                    }
                }
                SyncEvent::Cursor { account_id, cursor } => {
                    self.store.set_sync_state(&account_id, &cursor)?;
                }
                SyncEvent::Status { account_id, status } => {
                    if self.statuses.get(&account_id) != Some(&status) {
                        self.statuses.insert(account_id, status);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.reload()?;
        }
        Ok(changed)
    }

    pub fn sync_status(&self, account_id: &str) -> Option<&SyncStatus> {
        self.statuses.get(account_id)
    }

    /// Remove an account and everything stored for it on this device.
    pub fn remove_account(&mut self, account_id: &str) -> Result<()> {
        self.store.remove_account(account_id)?;
        self.statuses.remove(account_id);
        self.in_flight
            .retain(|_, item| item.account_id != account_id);
        self.reload()
    }
}

/// Days of mail fetched on an account's first sync. Older mail comes later.
pub const INITIAL_SYNC_DAYS: i64 = 90;
/// Most messages fetched per folder on an account's first sync.
pub const INITIAL_SYNC_LIMIT: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    /// The saved sign-in was rejected; the user has to sign in again.
    #[error("the account needs to be signed in again")]
    NeedsSignIn,
    /// Offline, rate limited or a server hiccup; the engine retries later.
    #[error("{0}")]
    Retry(String),
    /// The request itself is wrong; retrying won't help.
    #[error("{0}")]
    Failed(String),
}

/// Fetches a valid access token, refreshing it when needed.
pub type AccessToken = Box<dyn FnMut() -> std::result::Result<String, ProviderError> + Send>;

/// One mail service. Each account's background worker owns one.
pub trait MailProvider: Send {
    /// Bring the store up to date by emitting events. `cursor` is what the
    /// previous pass returned (`None` on the first sync). Returns the new cursor.
    fn sync(
        &mut self,
        cursor: Option<&str>,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> std::result::Result<String, ProviderError>;

    fn send(&mut self, raw: &[u8]) -> std::result::Result<(), ProviderError>;

    fn apply(&mut self, action: &crate::RemoteAction) -> std::result::Result<(), ProviderError>;
}
