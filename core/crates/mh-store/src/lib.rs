//! The encrypted local mail store.
//!
//! One SQLite database per user profile, encrypted with SQLCipher. The key
//! comes from the OS keychain and never touches disk in plain form.

use std::path::Path;

use mh_model::{
    Account, AccountId, Address, AttachmentInfo, Body, Flags, MessageHeader, MessageId,
    ProviderKind, ThreadState, Timestamp,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

mod schema;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("stored data is corrupt: {0}")]
    Corrupt(String),
    #[error("the database key is wrong or the file is not a MailHerd database")]
    BadKey,
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Standard folder roles, so the core can find "the inbox" in any provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Trash,
    Spam,
}

impl FolderRole {
    fn as_str(self) -> &'static str {
        match self {
            FolderRole::Inbox => "inbox",
            FolderRole::Sent => "sent",
            FolderRole::Drafts => "drafts",
            FolderRole::Archive => "archive",
            FolderRole::Trash => "trash",
            FolderRole::Spam => "spam",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub account_id: AccountId,
    pub provider_id: String,
    pub name: String,
    pub role: Option<FolderRole>,
}

/// A message to insert or update, keyed by (account, provider id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMessage {
    pub header: MessageHeader,
    pub body: Body,
    pub attachments: Vec<AttachmentInfo>,
    pub is_bulk: bool,
    pub folder_ids: Vec<i64>,
}

/// A queued action waiting to be sent to a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    pub id: i64,
    pub account_id: AccountId,
    pub kind: String,
    pub payload: String,
    pub not_before: Timestamp,
    pub attempts: u32,
}

pub struct Store {
    conn: Connection,
}

fn kind_to_str(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::ICloud => "icloud",
        ProviderKind::Gmail => "gmail",
        ProviderKind::Microsoft365 => "microsoft365",
    }
}

fn kind_from_str(s: &str) -> Result<ProviderKind> {
    match s {
        "icloud" => Ok(ProviderKind::ICloud),
        "gmail" => Ok(ProviderKind::Gmail),
        "microsoft365" => Ok(ProviderKind::Microsoft365),
        other => Err(StoreError::Corrupt(format!(
            "unknown provider kind {other}"
        ))),
    }
}

fn role_from_str(s: &str) -> Option<FolderRole> {
    Some(match s {
        "inbox" => FolderRole::Inbox,
        "sent" => FolderRole::Sent,
        "drafts" => FolderRole::Drafts,
        "archive" => FolderRole::Archive,
        "trash" => FolderRole::Trash,
        "spam" => FolderRole::Spam,
        _ => return None,
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| StoreError::Corrupt(e.to_string()))
}

fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

const HEADER_COLUMNS: &str = "id, account_id, provider_id, provider_thread_id, rfc_message_id, \
    in_reply_to, refs, subject, from_addr, to_addrs, cc_addrs, reply_to_addrs, date, \
    seen, flagged, answered, draft, has_attachments, snippet";

fn header_from_row(row: &Row<'_>) -> rusqlite::Result<MessageHeader> {
    Ok(MessageHeader {
        id: row.get(0)?,
        account_id: row.get(1)?,
        provider_id: row.get(2)?,
        provider_thread_id: row.get(3)?,
        rfc_message_id: row.get(4)?,
        in_reply_to: from_json(&row.get::<_, String>(5)?)?,
        references: from_json(&row.get::<_, String>(6)?)?,
        subject: row.get(7)?,
        from: from_json::<Address>(&row.get::<_, String>(8)?)?,
        to: from_json(&row.get::<_, String>(9)?)?,
        cc: from_json(&row.get::<_, String>(10)?)?,
        reply_to: from_json(&row.get::<_, String>(11)?)?,
        date: row.get(12)?,
        flags: Flags {
            seen: row.get(13)?,
            flagged: row.get(14)?,
            answered: row.get(15)?,
            draft: row.get(16)?,
        },
        has_attachments: row.get(17)?,
        snippet: row.get(18)?,
    })
}

impl Store {
    /// Open (or create) an encrypted database at `path`.
    pub fn open(path: &Path, key: &[u8; 32]) -> Result<Self> {
        let conn = Connection::open(path)?;
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex}'\";"))?;
        // A wrong key only shows up on the first real read.
        if conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
                r.get::<_, i64>(0)
            })
            .is_err()
        {
            return Err(StoreError::BadKey);
        }
        Self::init(conn)
    }

    /// An unencrypted in-memory database, for tests and the demo mode.
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: usize = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?
            .try_into()
            .unwrap_or(0);
        for (i, sql) in schema::MIGRATIONS.iter().enumerate().skip(version) {
            self.conn.execute_batch(&format!(
                "BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    // ---- accounts -------------------------------------------------------

    pub fn upsert_account(&self, account: &Account) -> Result<()> {
        self.conn.execute(
            "INSERT INTO accounts (id, kind, email, display_name, color) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET email = ?3, display_name = ?4, color = ?5",
            params![
                account.id,
                kind_to_str(account.kind),
                account.email,
                account.display_name,
                account.color
            ],
        )?;
        Ok(())
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, kind, email, display_name, color FROM accounts ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (id, kind, email, display_name, color) = row?;
            Ok(Account {
                id,
                kind: kind_from_str(&kind)?,
                email,
                display_name,
                color,
            })
        })
        .collect()
    }

    /// Opaque per-account sync position (history id, delta token, MODSEQ...).
    pub fn sync_state(&self, account_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT sync_state FROM accounts WHERE id = ?1",
                [account_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn set_sync_state(&self, account_id: &str, state: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE accounts SET sync_state = ?2 WHERE id = ?1",
            params![account_id, state],
        )?;
        Ok(())
    }

    // ---- folders --------------------------------------------------------

    pub fn upsert_folder(
        &self,
        account_id: &str,
        provider_id: &str,
        name: &str,
        role: Option<FolderRole>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO folders (account_id, provider_id, name, role) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, provider_id) DO UPDATE SET name = ?3, role = ?4",
            params![account_id, provider_id, name, role.map(FolderRole::as_str)],
        )?;
        Ok(self.conn.query_row(
            "SELECT id FROM folders WHERE account_id = ?1 AND provider_id = ?2",
            params![account_id, provider_id],
            |r| r.get(0),
        )?)
    }

    pub fn folders(&self, account_id: &str) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, provider_id, name, role FROM folders WHERE account_id = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(Folder {
                id: r.get(0)?,
                account_id: r.get(1)?,
                provider_id: r.get(2)?,
                name: r.get(3)?,
                role: r
                    .get::<_, Option<String>>(4)?
                    .as_deref()
                    .and_then(role_from_str),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn folder_by_role(&self, account_id: &str, role: FolderRole) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM folders WHERE account_id = ?1 AND role = ?2",
                params![account_id, role.as_str()],
                |r| r.get(0),
            )
            .optional()?)
    }

    // ---- messages -------------------------------------------------------

    /// Insert a message, or update it if this account already has it.
    pub fn upsert_message(&mut self, message: &NewMessage) -> Result<MessageId> {
        let h = &message.header;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO messages (account_id, provider_id, provider_thread_id, rfc_message_id,
                in_reply_to, refs, subject, from_addr, to_addrs, cc_addrs, reply_to_addrs, date,
                seen, flagged, answered, draft, has_attachments, snippet, body_text, body_html, is_bulk)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
             ON CONFLICT(account_id, provider_id) DO UPDATE SET
                provider_thread_id = ?3, seen = ?13, flagged = ?14, answered = ?15, draft = ?16,
                body_text = coalesce(?19, body_text), body_html = coalesce(?20, body_html)",
            params![
                h.account_id,
                h.provider_id,
                h.provider_thread_id,
                h.rfc_message_id,
                to_json(&h.in_reply_to)?,
                to_json(&h.references)?,
                h.subject,
                to_json(&h.from)?,
                to_json(&h.to)?,
                to_json(&h.cc)?,
                to_json(&h.reply_to)?,
                h.date,
                h.flags.seen,
                h.flags.flagged,
                h.flags.answered,
                h.flags.draft,
                h.has_attachments,
                h.snippet,
                message.body.text,
                message.body.html,
                message.is_bulk,
            ],
        )?;
        let id: MessageId = tx.query_row(
            "SELECT id FROM messages WHERE account_id = ?1 AND provider_id = ?2",
            params![h.account_id, h.provider_id],
            |r| r.get(0),
        )?;
        tx.execute("DELETE FROM message_folders WHERE message_id = ?1", [id])?;
        for folder in &message.folder_ids {
            tx.execute(
                "INSERT OR IGNORE INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                params![id, folder],
            )?;
        }
        tx.execute("DELETE FROM attachments WHERE message_id = ?1", [id])?;
        for a in &message.attachments {
            tx.execute(
                "INSERT INTO attachments (message_id, filename, mime_type, size, content_id, inline)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    a.filename,
                    a.mime_type,
                    i64::try_from(a.size).unwrap_or(i64::MAX),
                    a.content_id,
                    a.inline
                ],
            )?;
        }
        tx.commit()?;
        Ok(id)
    }

    /// Headers of every message in folders with `role`, across all accounts.
    pub fn headers_in_role(&self, role: FolderRole) -> Result<Vec<MessageHeader>> {
        let sql = format!(
            "SELECT {HEADER_COLUMNS} FROM messages WHERE id IN (
                SELECT mf.message_id FROM message_folders mf
                JOIN folders f ON f.id = mf.folder_id WHERE f.role = ?1)
             ORDER BY date"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([role.as_str()], header_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Headers of every stored message, oldest first.
    pub fn all_headers(&self) -> Result<Vec<MessageHeader>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {HEADER_COLUMNS} FROM messages ORDER BY date"
        ))?;
        let rows = stmt.query_map([], header_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn remove_account(&self, account_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM accounts WHERE id = ?1", [account_id])?;
        Ok(())
    }

    pub fn folder_id(&self, account_id: &str, provider_id: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM folders WHERE account_id = ?1 AND provider_id = ?2",
                params![account_id, provider_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn message_id(&self, account_id: &str, provider_id: &str) -> Result<Option<MessageId>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND provider_id = ?2",
                params![account_id, provider_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Replace the folders (or Gmail labels) a message is in.
    pub fn set_message_folders(&mut self, id: MessageId, folder_ids: &[i64]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM message_folders WHERE message_id = ?1", [id])?;
        for folder in folder_ids {
            tx.execute(
                "INSERT OR IGNORE INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                params![id, folder],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_message(&self, id: MessageId) -> Result<()> {
        self.conn
            .execute("DELETE FROM messages WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Remove the local copies kept for sent messages once the provider's copy has arrived.
    pub fn delete_local_copies(&self, account_id: &str, rfc_message_id: &str) -> Result<usize> {
        Ok(self.conn.execute(
            "DELETE FROM messages WHERE account_id = ?1 AND rfc_message_id = ?2 AND provider_id LIKE 'local:%'",
            params![account_id, rfc_message_id],
        )?)
    }

    /// Folder roles of every message that is in at least one role folder.
    pub fn message_roles(&self) -> Result<std::collections::HashMap<MessageId, Vec<FolderRole>>> {
        let mut stmt = self.conn.prepare(
            "SELECT mf.message_id, f.role FROM message_folders mf
             JOIN folders f ON f.id = mf.folder_id WHERE f.role IS NOT NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, MessageId>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut map: std::collections::HashMap<MessageId, Vec<FolderRole>> = Default::default();
        for row in rows {
            let (id, role) = row?;
            if let Some(role) = role_from_str(&role) {
                map.entry(id).or_default().push(role);
            }
        }
        Ok(map)
    }

    pub fn header(&self, id: MessageId) -> Result<Option<MessageHeader>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {HEADER_COLUMNS} FROM messages WHERE id = ?1"),
                [id],
                header_from_row,
            )
            .optional()?)
    }

    pub fn body(&self, id: MessageId) -> Result<Option<Body>> {
        Ok(self
            .conn
            .query_row(
                "SELECT body_text, body_html FROM messages WHERE id = ?1",
                [id],
                |r| {
                    Ok(Body {
                        text: r.get(0)?,
                        html: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn is_bulk(&self, id: MessageId) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT is_bulk FROM messages WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(false))
    }

    pub fn attachments(&self, id: MessageId) -> Result<Vec<AttachmentInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT filename, mime_type, size, content_id, inline FROM attachments WHERE message_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map([id], |r| {
            Ok(AttachmentInfo {
                filename: r.get(0)?,
                mime_type: r.get(1)?,
                size: usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                content_id: r.get(3)?,
                inline: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_flags(&self, id: MessageId, flags: Flags) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET seen = ?2, flagged = ?3, answered = ?4, draft = ?5 WHERE id = ?1",
            params![id, flags.seen, flags.flagged, flags.answered, flags.draft],
        )?;
        Ok(())
    }

    /// Move a message out of every folder with `from` role and into the `to` folder.
    pub fn move_to_role(&self, id: MessageId, from: FolderRole, to: Option<i64>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM message_folders WHERE message_id = ?1 AND folder_id IN
                (SELECT id FROM folders WHERE role = ?2)",
            params![id, from.as_str()],
        )?;
        if let Some(folder) = to {
            self.conn.execute(
                "INSERT OR IGNORE INTO message_folders (message_id, folder_id) VALUES (?1, ?2)",
                params![id, folder],
            )?;
        }
        Ok(())
    }

    // ---- thread state ---------------------------------------------------

    pub fn thread_state(&self, thread_id: &str) -> Result<ThreadState> {
        Ok(self
            .conn
            .query_row(
                "SELECT pinned, done, snoozed_until, remind_at FROM thread_state WHERE thread_id = ?1",
                [thread_id],
                |r| {
                    Ok(ThreadState {
                        pinned: r.get(0)?,
                        done: r.get(1)?,
                        snoozed_until: r.get(2)?,
                        remind_if_no_reply_at: r.get(3)?,
                    })
                },
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn set_thread_state(
        &self,
        thread_id: &str,
        state: ThreadState,
        now: Timestamp,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO thread_state (thread_id, pinned, done, snoozed_until, remind_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(thread_id) DO UPDATE SET pinned = ?2, done = ?3, snoozed_until = ?4,
                remind_at = ?5, updated_at = ?6",
            params![
                thread_id,
                state.pinned,
                state.done,
                state.snoozed_until,
                state.remind_if_no_reply_at,
                now
            ],
        )?;
        Ok(())
    }

    // ---- outbox ---------------------------------------------------------

    pub fn enqueue(
        &self,
        account_id: &str,
        kind: &str,
        payload: &str,
        not_before: Timestamp,
        now: Timestamp,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO outbox (account_id, kind, payload, not_before, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account_id, kind, payload, not_before, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Remove a queued item. Returns false if it was already gone (for example, already sent).
    pub fn dequeue(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM outbox WHERE id = ?1", [id])?
            > 0)
    }

    pub fn due_outbox(&self, now: Timestamp) -> Result<Vec<OutboxItem>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, kind, payload, not_before, attempts FROM outbox
             WHERE not_before <= ?1 ORDER BY not_before, id",
        )?;
        let rows = stmt.query_map([now], |r| {
            Ok(OutboxItem {
                id: r.get(0)?,
                account_id: r.get(1)?,
                kind: r.get(2)?,
                payload: r.get(3)?,
                not_before: r.get(4)?,
                attempts: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn pending_outbox(&self) -> Result<Vec<OutboxItem>> {
        self.due_outbox(Timestamp::MAX)
    }

    /// Record a failed attempt and push the next try back with exponential backoff.
    pub fn outbox_failed(&self, id: i64, error: &str, now: Timestamp) -> Result<()> {
        self.conn.execute(
            "UPDATE outbox SET attempts = attempts + 1, last_error = ?2,
                not_before = ?3 + min(3600, 30 * (1 << min(attempts, 7))) WHERE id = ?1",
            params![id, error, now],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> Account {
        Account {
            id: id.into(),
            kind: ProviderKind::Gmail,
            email: format!("{id}@example.org"),
            display_name: "Me".into(),
            color: 0x3366ff,
        }
    }

    fn message(account_id: &str, provider_id: &str, folder: i64) -> NewMessage {
        NewMessage {
            header: MessageHeader {
                id: 0,
                account_id: account_id.into(),
                provider_id: provider_id.into(),
                provider_thread_id: Some("T1".into()),
                rfc_message_id: Some(format!("{provider_id}@x")),
                in_reply_to: vec![],
                references: vec!["root@x".into()],
                subject: "Hello".into(),
                from: Address::new(Some("Ada"), "ada@example.org"),
                to: vec![Address::new(None, "me@example.org")],
                cc: vec![],
                reply_to: vec![],
                date: 1_000,
                flags: Flags::default(),
                has_attachments: true,
                snippet: "Hi there".into(),
            },
            body: Body {
                text: Some("Hi there".into()),
                html: None,
            },
            attachments: vec![AttachmentInfo {
                filename: Some("a.pdf".into()),
                mime_type: "application/pdf".into(),
                size: 10,
                content_id: None,
                inline: false,
            }],
            is_bulk: false,
            folder_ids: vec![folder],
        }
    }

    #[test]
    fn stores_and_reads_back_messages() -> Result<()> {
        let mut store = Store::open_in_memory()?;
        assert_eq!(store.schema_version()?, 1);
        store.upsert_account(&account("g"))?;
        let inbox = store.upsert_folder("g", "INBOX", "Inbox", Some(FolderRole::Inbox))?;
        let id = store.upsert_message(&message("g", "m1", inbox))?;

        let headers = store.headers_in_role(FolderRole::Inbox)?;
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].id, id);
        assert_eq!(headers[0].references, vec!["root@x"]);
        assert_eq!(headers[0].from.name.as_deref(), Some("Ada"));
        assert_eq!(
            store.body(id)?.and_then(|b| b.text).as_deref(),
            Some("Hi there")
        );
        assert_eq!(store.attachments(id)?.len(), 1);

        // Upserting the same provider id updates instead of duplicating.
        let mut again = message("g", "m1", inbox);
        again.header.flags.seen = true;
        assert_eq!(store.upsert_message(&again)?, id);
        assert_eq!(store.all_headers()?.len(), 1);
        assert!(store.header(id)?.is_some_and(|h| h.flags.seen));
        Ok(())
    }

    #[test]
    fn moving_out_of_the_inbox() -> Result<()> {
        let mut store = Store::open_in_memory()?;
        store.upsert_account(&account("g"))?;
        let inbox = store.upsert_folder("g", "INBOX", "Inbox", Some(FolderRole::Inbox))?;
        let archive = store.upsert_folder("g", "Archive", "Archive", Some(FolderRole::Archive))?;
        let id = store.upsert_message(&message("g", "m1", inbox))?;
        store.move_to_role(id, FolderRole::Inbox, Some(archive))?;
        assert!(store.headers_in_role(FolderRole::Inbox)?.is_empty());
        assert_eq!(store.headers_in_role(FolderRole::Archive)?.len(), 1);
        Ok(())
    }

    #[test]
    fn thread_state_round_trips() -> Result<()> {
        let store = Store::open_in_memory()?;
        assert_eq!(store.thread_state("t:1")?, ThreadState::default());
        let state = ThreadState {
            pinned: true,
            snoozed_until: Some(5_000),
            ..ThreadState::default()
        };
        store.set_thread_state("t:1", state, 100)?;
        assert_eq!(store.thread_state("t:1")?, state);
        Ok(())
    }

    #[test]
    fn outbox_due_dequeue_and_backoff() -> Result<()> {
        let store = Store::open_in_memory()?;
        store.upsert_account(&account("g"))?;
        let id = store.enqueue("g", "send", "{}", 110, 100)?;
        assert!(store.due_outbox(105)?.is_empty());
        assert_eq!(store.due_outbox(110)?.len(), 1);
        store.outbox_failed(id, "offline", 110)?;
        assert!(store.due_outbox(130)?.is_empty());
        assert_eq!(store.due_outbox(140)?[0].attempts, 1);
        assert!(store.dequeue(id)?);
        assert!(!store.dequeue(id)?);
        Ok(())
    }

    #[test]
    fn encrypted_database_needs_the_right_key() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("mh-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        let path = dir.join("mail.db");
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path, &[7u8; 32])?;
            store.upsert_account(&account("g"))?;
        }
        let reopened = Store::open(&path, &[7u8; 32])?;
        assert_eq!(reopened.accounts()?.len(), 1);
        drop(reopened);
        assert!(matches!(
            Store::open(&path, &[8u8; 32]),
            Err(StoreError::BadKey)
        ));
        let raw = std::fs::read(&path).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        assert!(
            !raw.windows(15).any(|w| w == b"SQLite format 3"),
            "file must be encrypted"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
