//! Database schema and migrations. Each entry upgrades the schema by one version;
//! never edit a migration that has shipped, add a new one instead.

pub const MIGRATIONS: &[&str] = &[
    // Version 1
    r#"
    CREATE TABLE accounts (
        id            TEXT PRIMARY KEY,
        kind          TEXT NOT NULL,
        email         TEXT NOT NULL,
        display_name  TEXT NOT NULL,
        color         INTEGER NOT NULL,
        sync_state    TEXT
    );

    CREATE TABLE folders (
        id           INTEGER PRIMARY KEY,
        account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        provider_id  TEXT NOT NULL,
        name         TEXT NOT NULL,
        role         TEXT,
        UNIQUE (account_id, provider_id)
    );

    CREATE TABLE messages (
        id                  INTEGER PRIMARY KEY,
        account_id          TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        provider_id         TEXT NOT NULL,
        provider_thread_id  TEXT,
        rfc_message_id      TEXT,
        in_reply_to         TEXT NOT NULL,
        refs                TEXT NOT NULL,
        subject             TEXT NOT NULL,
        from_addr           TEXT NOT NULL,
        to_addrs            TEXT NOT NULL,
        cc_addrs            TEXT NOT NULL,
        reply_to_addrs      TEXT NOT NULL,
        date                INTEGER NOT NULL,
        seen                INTEGER NOT NULL DEFAULT 0,
        flagged             INTEGER NOT NULL DEFAULT 0,
        answered            INTEGER NOT NULL DEFAULT 0,
        draft               INTEGER NOT NULL DEFAULT 0,
        has_attachments     INTEGER NOT NULL DEFAULT 0,
        snippet             TEXT NOT NULL,
        body_text           TEXT,
        body_html           TEXT,
        is_bulk             INTEGER NOT NULL DEFAULT 0,
        UNIQUE (account_id, provider_id)
    );
    CREATE INDEX messages_rfc_id ON messages(rfc_message_id);
    CREATE INDEX messages_date ON messages(date);

    CREATE TABLE message_folders (
        message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
        folder_id   INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
        PRIMARY KEY (message_id, folder_id)
    );

    CREATE TABLE attachments (
        id          INTEGER PRIMARY KEY,
        message_id  INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
        filename    TEXT,
        mime_type   TEXT NOT NULL,
        size        INTEGER NOT NULL,
        content_id  TEXT,
        inline      INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX attachments_message ON attachments(message_id);

    CREATE TABLE thread_state (
        thread_id       TEXT PRIMARY KEY,
        pinned          INTEGER NOT NULL DEFAULT 0,
        done            INTEGER NOT NULL DEFAULT 0,
        snoozed_until   INTEGER,
        remind_at       INTEGER,
        updated_at      INTEGER NOT NULL
    );

    CREATE TABLE outbox (
        id          INTEGER PRIMARY KEY,
        account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
        kind        TEXT NOT NULL,
        payload     TEXT NOT NULL,
        not_before  INTEGER NOT NULL,
        attempts    INTEGER NOT NULL DEFAULT 0,
        last_error  TEXT,
        created_at  INTEGER NOT NULL
    );
    CREATE INDEX outbox_due ON outbox(not_before);
    "#,
];
