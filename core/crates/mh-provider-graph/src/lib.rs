//! Microsoft 365 mail through Microsoft Graph.
//!
//! - Inbox, Sent Items and Archive are synced with per-folder delta queries.
//!   The first pass covers the last [`INITIAL_SYNC_DAYS`] days.
//! - Messages are downloaded as MIME (`/$value`) so parsing matches every
//!   other provider.
//! - Immutable ids are requested, so a message keeps its id when it moves
//!   between folders; a move shows up as "removed here, present there".

use std::collections::{BTreeMap, HashMap, HashSet};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
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

pub const API_BASE: &str = "https://graph.microsoft.com/v1.0";

/// Well-known folders: (Graph name, role, synced).
const WELL_KNOWN: &[(&str, FolderRole, bool)] = &[
    ("inbox", FolderRole::Inbox, true),
    ("sentitems", FolderRole::Sent, true),
    ("archive", FolderRole::Archive, true),
    ("drafts", FolderRole::Drafts, false),
    ("deleteditems", FolderRole::Trash, false),
    ("junkemail", FolderRole::Spam, false),
];

pub struct Graph {
    account_id: AccountId,
    base: String,
    http: Http,
    token: AccessToken,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cursor {
    /// Folder id -> delta link for the next pass.
    #[serde(default)]
    delta: BTreeMap<String, String>,
    /// Messages already downloaded, so later changes update them instead of
    /// downloading them again.
    #[serde(default)]
    known: HashSet<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Me {
    mail: Option<String>,
    user_principal_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Folder {
    id: String,
    display_name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct FlagStatus {
    flag_status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeltaItem {
    id: String,
    conversation_id: Option<String>,
    is_read: Option<bool>,
    is_draft: Option<bool>,
    #[serde(default)]
    flag: FlagStatus,
    received_date_time: Option<String>,
    #[serde(rename = "@removed")]
    removed: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct DeltaPage {
    #[serde(default)]
    value: Vec<DeltaItem>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    delta_link: Option<String>,
}

fn provider_error(e: HttpError) -> ProviderError {
    match e {
        HttpError::Unauthorized => ProviderError::NeedsSignIn,
        HttpError::Retry(m) => ProviderError::Retry(m),
        HttpError::Failed { status, body } => {
            ProviderError::Failed(format!("Microsoft 365 {status}: {body}"))
        }
        HttpError::Decode(m) => {
            ProviderError::Failed(format!("Microsoft 365 sent an unexpected reply: {m}"))
        }
    }
}

fn segment(id: &str) -> String {
    form_urlencoded::byte_serialize(id.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

fn parse_time(s: &str) -> i64 {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map(|t| t.unix_timestamp())
        .unwrap_or(0)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

/// One message as seen in this pass: where it is and its state.
struct Present {
    folder: String,
    conversation_id: Option<String>,
    flags: Flags,
    received_at: i64,
}

/// One folder's delta: messages present, ids removed, and the next delta link.
type Delta = (Vec<(String, Present)>, Vec<String>, String);

impl Graph {
    pub fn new(account_id: AccountId, token: AccessToken) -> Self {
        Self::with_base(account_id, token, API_BASE)
    }

    pub fn with_base(account_id: AccountId, token: AccessToken, base: &str) -> Self {
        Self {
            account_id,
            base: base.trim_end_matches('/').to_string(),
            http: Http::new()
                .with_header("Prefer", "IdType=\"ImmutableId\", odata.maxpagesize=100"),
            token,
        }
    }

    fn url(&self, path_or_url: &str) -> String {
        if path_or_url.starts_with("http") {
            path_or_url.to_string()
        } else {
            format!("{}{path_or_url}", self.base)
        }
    }

    fn get<T: serde::de::DeserializeOwned>(&mut self, path: &str) -> Result<T, ProviderError> {
        let token = (self.token)()?;
        self.http
            .get_json(&self.url(path), &token)
            .map_err(provider_error)
    }

    fn send_json(
        &mut self,
        method: &str,
        path: &str,
        body: serde_json::Value,
    ) -> Result<(), ProviderError> {
        let token = (self.token)()?;
        self.http
            .send_json::<_, serde_json::Value>(method, &self.url(path), &token, &body)
            .map(|_| ())
            .map_err(provider_error)
    }

    /// The signed-in address, used when the account is first added.
    pub fn email_address(&mut self) -> Result<String, ProviderError> {
        let me: Me = self.get("/me")?;
        me.mail.or(me.user_principal_name).ok_or_else(|| {
            ProviderError::Failed("Microsoft 365 didn't say which address this is".into())
        })
    }

    /// The well-known folders that exist for this mailbox, with their ids.
    fn folders(&mut self) -> Result<Vec<(RemoteFolder, bool)>, ProviderError> {
        let mut out = Vec::new();
        for (name, role, synced) in WELL_KNOWN {
            match self.get::<Folder>(&format!("/me/mailFolders/{name}")) {
                Ok(f) => out.push((
                    RemoteFolder {
                        provider_id: f.id,
                        name: f.display_name,
                        role: Some(*role),
                    },
                    *synced,
                )),
                // Not every mailbox has an Archive folder.
                Err(ProviderError::Failed(m)) if m.starts_with("Microsoft 365 404") => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// Follow one folder's delta pages.
    fn delta(&mut self, folder_id: &str, link: Option<&str>) -> Result<Delta, ProviderError> {
        let mut next = match link {
            Some(l) => l.to_string(),
            None => {
                let since =
                    time::OffsetDateTime::from_unix_timestamp(now() - INITIAL_SYNC_DAYS * 86_400)
                        .ok()
                        .and_then(|t| {
                            t.format(&time::format_description::well_known::Rfc3339)
                                .ok()
                        })
                        .unwrap_or_default();
                let filter: String = form_urlencoded::byte_serialize(
                    format!("receivedDateTime ge {since}").as_bytes(),
                )
                .collect();
                format!(
                    "/me/mailFolders/{}/messages/delta?$select=id,conversationId,isRead,isDraft,flag,receivedDateTime&$filter={filter}",
                    segment(folder_id)
                )
            }
        };
        let mut present = Vec::new();
        let mut removed = Vec::new();
        loop {
            let page: DeltaPage = self.get(&next)?;
            for item in page.value {
                if item.removed.is_some() {
                    removed.push(item.id);
                    continue;
                }
                present.push((
                    item.id,
                    Present {
                        folder: folder_id.to_string(),
                        conversation_id: item.conversation_id,
                        flags: Flags {
                            seen: item.is_read.unwrap_or(false),
                            flagged: item.flag.flag_status.as_deref() == Some("flagged"),
                            answered: false,
                            draft: item.is_draft.unwrap_or(false),
                        },
                        received_at: item.received_date_time.as_deref().map_or(0, parse_time),
                    },
                ));
            }
            match (page.next_link, page.delta_link) {
                (Some(n), _) => next = n,
                (None, Some(d)) => return Ok((present, removed, d)),
                (None, None) => {
                    return Err(ProviderError::Retry(
                        "Microsoft 365 delta ended without a link".into(),
                    ));
                }
            }
        }
    }

    fn status(&self, status: SyncStatus, emit: &mut dyn FnMut(SyncEvent)) {
        emit(SyncEvent::Status {
            account_id: self.account_id.clone(),
            status,
        });
    }

    fn move_to(&mut self, ids: &[String], destination: &str) -> Result<(), ProviderError> {
        for id in ids.iter().filter(|id| !id.starts_with("local:")) {
            self.send_json(
                "POST",
                &format!("/me/messages/{}/move", segment(id)),
                json!({ "destinationId": destination }),
            )?;
        }
        Ok(())
    }

    fn set_read(&mut self, ids: &[String], read: bool) -> Result<(), ProviderError> {
        for id in ids.iter().filter(|id| !id.starts_with("local:")) {
            self.send_json(
                "PATCH",
                &format!("/me/messages/{}", segment(id)),
                json!({ "isRead": read }),
            )?;
        }
        Ok(())
    }
}

impl MailProvider for Graph {
    fn sync(
        &mut self,
        cursor: Option<&str>,
        emit: &mut dyn FnMut(SyncEvent),
    ) -> Result<String, ProviderError> {
        self.status(SyncStatus::Connecting, emit);
        let mut state: Cursor = cursor
            .and_then(|c| serde_json::from_str(c).ok())
            .unwrap_or_default();
        let folders = self.folders()?;
        emit(SyncEvent::Folders {
            account_id: self.account_id.clone(),
            folders: folders.iter().map(|(f, _)| f.clone()).collect(),
        });

        // Gather every synced folder's changes first: a move is a removal in
        // one folder and an appearance in another, in either order.
        let mut present: HashMap<String, Present> = HashMap::new();
        let mut removed: HashSet<String> = HashSet::new();
        let mut links = BTreeMap::new();
        for (folder, _) in folders.iter().filter(|(_, synced)| *synced) {
            let link = state.delta.get(&folder.provider_id).cloned();
            let (seen, gone, next) = self.delta(&folder.provider_id, link.as_deref())?;
            present.extend(seen);
            removed.extend(gone);
            links.insert(folder.provider_id.clone(), next);
        }

        let mut to_fetch: Vec<(String, Present)> = Vec::new();
        for (id, p) in present.drain() {
            removed.remove(&id);
            if state.known.contains(&id) {
                emit(SyncEvent::State {
                    account_id: self.account_id.clone(),
                    provider_id: id,
                    flags: Some(p.flags),
                    folders: Some(vec![p.folder]),
                });
            } else {
                to_fetch.push((id, p));
            }
        }
        for id in removed {
            state.known.remove(&id);
            emit(SyncEvent::Removed {
                account_id: self.account_id.clone(),
                provider_id: id,
            });
        }

        // Newest first, so the inbox fills in from the top.
        to_fetch.sort_by_key(|(_, p)| std::cmp::Reverse(p.received_at));
        to_fetch.truncate(INITIAL_SYNC_LIMIT * 3);
        let total = to_fetch.len();
        for (done, (id, p)) in to_fetch.into_iter().enumerate() {
            if done % 25 == 0 && total > 25 {
                self.status(
                    SyncStatus::Syncing {
                        done,
                        total: Some(total),
                    },
                    emit,
                );
            }
            let token = (self.token)()?;
            let raw = match self.http.get_bytes(
                &self.url(&format!("/me/messages/{}/$value", segment(&id))),
                &token,
            ) {
                Ok(raw) => raw,
                Err(HttpError::Failed { status: 404, .. }) => continue,
                Err(e) => return Err(provider_error(e)),
            };
            emit(SyncEvent::Message {
                account_id: self.account_id.clone(),
                provider_id: id.clone(),
                provider_thread_id: p.conversation_id,
                raw,
                folders: vec![p.folder],
                flags: p.flags,
                received_at: p.received_at,
            });
            state.known.insert(id);
        }

        state.delta = links;
        self.status(SyncStatus::UpToDate { at: now() }, emit);
        serde_json::to_string(&state).map_err(|e| ProviderError::Failed(e.to_string()))
    }

    fn send(&mut self, raw: &[u8]) -> Result<(), ProviderError> {
        // Graph accepts a whole MIME message, base64-encoded, as text/plain.
        // It files the copy in Sent Items itself.
        let token = (self.token)()?;
        self.http
            .post_raw(
                &self.url("/me/sendMail"),
                &token,
                "text/plain",
                STANDARD.encode(raw).as_bytes(),
            )
            .map(|_| ())
            .map_err(provider_error)
    }

    fn apply(&mut self, action: &RemoteAction) -> Result<(), ProviderError> {
        match action {
            RemoteAction::MarkRead { provider_ids } => self.set_read(provider_ids, true),
            RemoteAction::MarkUnread { provider_ids } => self.set_read(provider_ids, false),
            RemoteAction::Archive { provider_ids } => self.move_to(provider_ids, "archive"),
            RemoteAction::MoveToInbox { provider_ids } => self.move_to(provider_ids, "inbox"),
            RemoteAction::SetThreadState { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
