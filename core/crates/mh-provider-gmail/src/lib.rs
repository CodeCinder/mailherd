//! Gmail through the Gmail REST API.
//!
//! - First sync: the last [`INITIAL_SYNC_DAYS`] days of mail (up to
//!   [`INITIAL_SYNC_LIMIT`] messages), fetched as raw RFC 5322 so parsing is
//!   identical for every provider.
//! - Later syncs: the history feed from the saved `historyId`, which reports
//!   new messages, label changes and deletions.
//! - Labels map to folders; mail with no Inbox label is "archived".

use base64::Engine;
use base64::alphabet::URL_SAFE;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use mh_app::RemoteAction;
use mh_app::sync::{
    AccessToken, INITIAL_SYNC_DAYS, INITIAL_SYNC_LIMIT, MailProvider, ProviderError, RemoteFolder,
    SyncEvent, SyncStatus,
};
use mh_auth::http::{Http, HttpError};
use mh_model::{AccountId, Flags};
use mh_store::FolderRole;
use serde::{Deserialize, Serialize};
use serde_json::json;
use url::form_urlencoded;

pub const API_BASE: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

/// Gmail has no Archive label: archived mail is mail without INBOX.
pub const ARCHIVE_FOLDER: &str = "__archive";

const PAGE_SIZE: usize = 500;

/// Gmail's base64url sometimes has padding and sometimes doesn't.
const B64: GeneralPurpose = GeneralPurpose::new(
    &URL_SAFE,
    GeneralPurposeConfig::new()
        .with_encode_padding(false)
        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

pub struct Gmail {
    account_id: AccountId,
    base: String,
    http: Http,
    token: AccessToken,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct Cursor {
    history_id: String,
}

#[derive(Deserialize)]
struct Label {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Deserialize)]
struct Labels {
    #[serde(default)]
    labels: Vec<Label>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    email_address: String,
    history_id: String,
}

#[derive(Deserialize)]
struct MessageRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageList {
    #[serde(default)]
    messages: Vec<MessageRef>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMessage {
    id: String,
    thread_id: Option<String>,
    #[serde(default)]
    label_ids: Vec<String>,
    internal_date: Option<String>,
    raw: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryMessage {
    id: String,
    #[serde(default)]
    label_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct HistoryItem {
    message: HistoryMessage,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HistoryRecord {
    #[serde(default)]
    messages_added: Vec<HistoryItem>,
    #[serde(default)]
    labels_added: Vec<HistoryItem>,
    #[serde(default)]
    labels_removed: Vec<HistoryItem>,
    #[serde(default)]
    messages_deleted: Vec<HistoryItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPage {
    #[serde(default)]
    history: Vec<HistoryRecord>,
    next_page_token: Option<String>,
    history_id: Option<String>,
}

fn role_for(label: &str) -> Option<FolderRole> {
    Some(match label {
        "INBOX" => FolderRole::Inbox,
        "SENT" => FolderRole::Sent,
        "DRAFT" => FolderRole::Drafts,
        "TRASH" => FolderRole::Trash,
        "SPAM" => FolderRole::Spam,
        _ => return None,
    })
}

/// System labels that are states, not places (UNREAD, STARRED, categories...).
fn is_folder_label(label: &Label) -> bool {
    label.kind == "user" || role_for(&label.id).is_some()
}

/// Folders and flags for a message from its Gmail labels.
fn placement(labels: &[String]) -> (Vec<String>, Flags) {
    let has = |l: &str| labels.iter().any(|x| x == l);
    let mut folders: Vec<String> = labels
        .iter()
        .filter(|l| role_for(l).is_some() || l.starts_with("Label_"))
        .cloned()
        .collect();
    if !["INBOX", "SENT", "DRAFT", "TRASH", "SPAM"]
        .iter()
        .any(|l| has(l))
    {
        folders.push(ARCHIVE_FOLDER.to_string());
    }
    let flags = Flags {
        seen: !has("UNREAD"),
        flagged: has("STARRED"),
        answered: false,
        draft: has("DRAFT"),
    };
    (folders, flags)
}

fn provider_error(e: HttpError) -> ProviderError {
    match e {
        HttpError::Unauthorized => ProviderError::NeedsSignIn,
        HttpError::Retry(m) => ProviderError::Retry(m),
        HttpError::Failed { status, body } => {
            ProviderError::Failed(format!("Gmail {status}: {body}"))
        }
        HttpError::Decode(m) => {
            ProviderError::Failed(format!("Gmail sent an unexpected reply: {m}"))
        }
    }
}

fn encode(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

impl Gmail {
    pub fn new(account_id: AccountId, token: AccessToken) -> Self {
        Self::with_base(account_id, token, API_BASE)
    }

    pub fn with_base(account_id: AccountId, token: AccessToken, base: &str) -> Self {
        Self {
            account_id,
            base: base.trim_end_matches('/').to_string(),
            http: Http::new(),
            token,
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&mut self, path: &str) -> Result<T, ProviderError> {
        let token = (self.token)()?;
        self.http
            .get_json(&format!("{}{path}", self.base), &token)
            .map_err(provider_error)
    }

    fn post(&mut self, path: &str, body: serde_json::Value) -> Result<(), ProviderError> {
        let token = (self.token)()?;
        self.http
            .send_json::<_, serde_json::Value>(
                "POST",
                &format!("{}{path}", self.base),
                &token,
                &body,
            )
            .map(|_| ())
            .map_err(provider_error)
    }

    /// The signed-in address, used when the account is first added.
    pub fn email_address(&mut self) -> Result<String, ProviderError> {
        Ok(self.get::<Profile>("/profile")?.email_address)
    }

    fn folders(&mut self) -> Result<Vec<RemoteFolder>, ProviderError> {
        let labels: Labels = self.get("/labels")?;
        let mut folders: Vec<RemoteFolder> = labels
            .labels
            .into_iter()
            .filter(is_folder_label)
            .map(|l| RemoteFolder {
                role: role_for(&l.id),
                name: match l.id.as_str() {
                    "INBOX" => "Inbox".into(),
                    "SENT" => "Sent".into(),
                    "DRAFT" => "Drafts".into(),
                    "TRASH" => "Trash".into(),
                    "SPAM" => "Spam".into(),
                    _ => l.name,
                },
                provider_id: l.id,
            })
            .collect();
        folders.push(RemoteFolder {
            provider_id: ARCHIVE_FOLDER.into(),
            name: "Archive".into(),
            role: Some(FolderRole::Archive),
        });
        Ok(folders)
    }

    fn fetch(&mut self, id: &str, emit: &mut dyn FnMut(SyncEvent)) -> Result<(), ProviderError> {
        let msg: RawMessage = match self.get(&format!("/messages/{}?format=raw", encode(id))) {
            Ok(m) => m,
            // Deleted between listing and fetching: nothing to do.
            Err(ProviderError::Failed(m)) if m.starts_with("Gmail 404") => return Ok(()),
            Err(e) => return Err(e),
        };
        let raw = B64
            .decode(msg.raw.trim())
            .map_err(|e| ProviderError::Failed(format!("Gmail message {id} is not base64: {e}")))?;
        let (folders, flags) = placement(&msg.label_ids);
        let received_at = msg
            .internal_date
            .and_then(|d| d.parse::<i64>().ok())
            .map_or(0, |ms| ms / 1000);
        emit(SyncEvent::Message {
            account_id: self.account_id.clone(),
            provider_id: msg.id,
            provider_thread_id: msg.thread_id,
            raw,
            folders,
            flags,
            received_at,
        });
        Ok(())
    }

    fn status(&self, status: SyncStatus, emit: &mut dyn FnMut(SyncEvent)) {
        emit(SyncEvent::Status {
            account_id: self.account_id.clone(),
            status,
        });
    }

    fn full_sync(&mut self, emit: &mut dyn FnMut(SyncEvent)) -> Result<String, ProviderError> {
        // Take the history position first so nothing that arrives during the
        // download is missed; the next incremental pass picks it up.
        let profile: Profile = self.get("/profile")?;
        let query = encode(&format!(
            "newer_than:{INITIAL_SYNC_DAYS}d -in:spam -in:trash"
        ));
        let mut ids = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut path = format!("/messages?maxResults={PAGE_SIZE}&q={query}");
            if let Some(token) = &page {
                path.push_str(&format!("&pageToken={}", encode(token)));
            }
            let list: MessageList = self.get(&path)?;
            ids.extend(list.messages.into_iter().map(|m| m.id));
            page = list.next_page_token;
            if page.is_none() || ids.len() >= INITIAL_SYNC_LIMIT {
                break;
            }
        }
        ids.truncate(INITIAL_SYNC_LIMIT);
        let total = ids.len();
        for (done, id) in ids.iter().enumerate() {
            if done % 25 == 0 {
                self.status(
                    SyncStatus::Syncing {
                        done,
                        total: Some(total),
                    },
                    emit,
                );
            }
            self.fetch(id, emit)?;
        }
        Ok(profile.history_id)
    }

    fn incremental(
        &mut self,
        history_id: &str,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> Result<Option<String>, ProviderError> {
        let mut page: Option<String> = None;
        let mut latest = history_id.to_string();
        let mut added: Vec<String> = Vec::new();
        loop {
            let mut path = format!(
                "/history?startHistoryId={}&historyTypes=messageAdded&historyTypes=labelAdded&historyTypes=labelRemoved&historyTypes=messageDeleted",
                encode(history_id)
            );
            if let Some(token) = &page {
                path.push_str(&format!("&pageToken={}", encode(token)));
            }
            let result: Result<HistoryPage, ProviderError> = self.get(&path);
            let history = match result {
                Ok(h) => h,
                // The saved position is too old: start over.
                Err(ProviderError::Failed(m)) if m.starts_with("Gmail 404") => return Ok(None),
                Err(e) => return Err(e),
            };
            for record in history.history {
                for item in record.messages_added {
                    let labels = item.message.label_ids.unwrap_or_default();
                    if !labels.iter().any(|l| l == "SPAM" || l == "TRASH")
                        && !added.contains(&item.message.id)
                    {
                        added.push(item.message.id);
                    }
                }
                for item in record.labels_added.into_iter().chain(record.labels_removed) {
                    if let Some(labels) = item.message.label_ids {
                        let (folders, flags) = placement(&labels);
                        emit(SyncEvent::State {
                            account_id: self.account_id.clone(),
                            provider_id: item.message.id,
                            flags: Some(flags),
                            folders: Some(folders),
                        });
                    }
                }
                for item in record.messages_deleted {
                    added.retain(|id| id != &item.message.id);
                    emit(SyncEvent::Removed {
                        account_id: self.account_id.clone(),
                        provider_id: item.message.id,
                    });
                }
            }
            if let Some(id) = history.history_id {
                latest = id;
            }
            page = history.next_page_token;
            if page.is_none() {
                break;
            }
        }
        for id in &added {
            self.fetch(id, emit)?;
        }
        Ok(Some(latest))
    }

    fn modify(
        &mut self,
        ids: &[String],
        add: &[&str],
        remove: &[&str],
    ) -> Result<(), ProviderError> {
        // Local copies of sent mail ("local:…") don't exist on the server yet.
        let ids: Vec<&String> = ids.iter().filter(|id| !id.starts_with("local:")).collect();
        if ids.is_empty() {
            return Ok(());
        }
        self.post(
            "/messages/batchModify",
            json!({ "ids": ids, "addLabelIds": add, "removeLabelIds": remove }),
        )
    }
}

impl MailProvider for Gmail {
    fn sync(
        &mut self,
        cursor: Option<&str>,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> Result<String, ProviderError> {
        self.status(SyncStatus::Connecting, emit);
        let folders = self.folders()?;
        emit(SyncEvent::Folders {
            account_id: self.account_id.clone(),
            folders,
        });
        let previous = cursor.and_then(|c| serde_json::from_str::<Cursor>(c).ok());
        let history_id = match previous {
            Some(c) => match self.incremental(&c.history_id, emit)? {
                Some(h) => h,
                None => self.full_sync(emit)?,
            },
            None => self.full_sync(emit)?,
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
            .unwrap_or(0);
        self.status(SyncStatus::UpToDate { at: now }, emit);
        serde_json::to_string(&Cursor { history_id })
            .map_err(|e| ProviderError::Failed(e.to_string()))
    }

    fn send(&mut self, raw: &[u8]) -> Result<(), ProviderError> {
        self.post("/messages/send", json!({ "raw": B64.encode(raw) }))
    }

    fn apply(&mut self, action: &RemoteAction) -> Result<(), ProviderError> {
        match action {
            RemoteAction::MarkRead { provider_ids } => self.modify(provider_ids, &[], &["UNREAD"]),
            RemoteAction::MarkUnread { provider_ids } => {
                self.modify(provider_ids, &["UNREAD"], &[])
            }
            RemoteAction::Archive { provider_ids } => self.modify(provider_ids, &[], &["INBOX"]),
            RemoteAction::MoveToInbox { provider_ids } => {
                self.modify(provider_ids, &["INBOX"], &[])
            }
            // Pin/Done/snooze state is stored in the mailbox in v0.2.
            RemoteAction::SetThreadState { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
