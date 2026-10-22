//! An offline demo: three sample accounts and a handful of realistic
//! conversations, so the UI can be tried before any real account is connected.
//! Every person, address and message here is made up.

use mh_model::{Account, Flags, ProviderKind, Timestamp};
use mh_store::{FolderRole, Store};

use crate::{Core, Incoming, JobResult, OutboxJob, RemoteAction, Result, Transport, Work};

/// Pretends to send: every job succeeds as soon as it is submitted.
#[derive(Debug, Default)]
pub struct DemoTransport {
    pub sent: Vec<Vec<u8>>,
    pub actions: Vec<RemoteAction>,
    done: Vec<JobResult>,
}

impl Transport for DemoTransport {
    fn submit(&mut self, job: OutboxJob) {
        match job.work {
            Work::Send { raw } => self.sent.push(raw),
            Work::Apply(action) => self.actions.push(action),
        }
        self.done.push(JobResult {
            id: job.id,
            result: Ok(()),
        });
    }

    fn poll(&mut self) -> Vec<JobResult> {
        std::mem::take(&mut self.done)
    }
}

pub const ICLOUD: &str = "demo-icloud";
pub const GMAIL: &str = "demo-gmail";
pub const WORK: &str = "demo-work";

const ME_ICLOUD: &str = "you@icloud.example";
const ME_GMAIL: &str = "you@gmail.example";
const ME_WORK: &str = "you@contoso.example";

const HOUR: Timestamp = 3_600;
const DAY: Timestamp = 86_400;

#[derive(Default)]
struct Mail<'a> {
    id: &'a str,
    refs: &'a [&'a str],
    from: &'a str,
    to: &'a str,
    cc: &'a str,
    subject: &'a str,
    ago: Timestamp,
    body: &'a str,
    html: Option<&'a str>,
    extra_headers: &'a str,
    attachment: Option<(&'a str, &'a str)>,
}

fn rfc2822(ts: Timestamp) -> String {
    time::OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .and_then(|dt| {
            dt.format(&time::format_description::well_known::Rfc2822)
                .ok()
        })
        .unwrap_or_else(|| "Thu, 01 Jan 1970 00:00:00 +0000".into())
}

fn raw(mail: &Mail<'_>, now: Timestamp) -> Vec<u8> {
    let mut out = String::new();
    out.push_str(&format!("Message-ID: <{}>\r\n", mail.id));
    if let Some(parent) = mail.refs.last() {
        out.push_str(&format!("In-Reply-To: <{parent}>\r\n"));
        let refs: Vec<String> = mail.refs.iter().map(|r| format!("<{r}>")).collect();
        out.push_str(&format!("References: {}\r\n", refs.join(" ")));
    }
    out.push_str(&format!("From: {}\r\nTo: {}\r\n", mail.from, mail.to));
    if !mail.cc.is_empty() {
        out.push_str(&format!("Cc: {}\r\n", mail.cc));
    }
    out.push_str(&format!(
        "Subject: {}\r\nDate: {}\r\nMIME-Version: 1.0\r\n",
        mail.subject,
        rfc2822(now - mail.ago)
    ));
    out.push_str(mail.extra_headers);
    let body = mail.body.replace('\n', "\r\n");
    match (mail.html, mail.attachment) {
        (Some(html), _) => {
            out.push_str("Content-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n");
            out.push_str("--alt\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
            out.push_str(&body);
            out.push_str("\r\n--alt\r\nContent-Type: text/html; charset=utf-8\r\n\r\n");
            out.push_str(html);
            out.push_str("\r\n--alt--\r\n");
        }
        (None, Some((name, mime))) => {
            out.push_str("Content-Type: multipart/mixed; boundary=\"mix\"\r\n\r\n");
            out.push_str("--mix\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
            out.push_str(&body);
            out.push_str(&format!(
                "\r\n--mix\r\nContent-Type: {mime}\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQKJcfsj6IKMSAwIG9iago8PC9UeXBlL0NhdGFsb2c+PgplbmRvYmoK\r\n--mix--\r\n"
            ));
        }
        (None, None) => {
            out.push_str("Content-Type: text/plain; charset=utf-8\r\n\r\n");
            out.push_str(&body);
            out.push_str("\r\n");
        }
    }
    out.into_bytes()
}

fn demo_accounts() -> [Account; 3] {
    [
        Account {
            id: ICLOUD.into(),
            kind: ProviderKind::ICloud,
            email: ME_ICLOUD.into(),
            display_name: "You".into(),
            color: 0x34aadc,
        },
        Account {
            id: GMAIL.into(),
            kind: ProviderKind::Gmail,
            email: ME_GMAIL.into(),
            display_name: "You".into(),
            color: 0xea4335,
        },
        Account {
            id: WORK.into(),
            kind: ProviderKind::Microsoft365,
            email: ME_WORK.into(),
            display_name: "You".into(),
            color: 0x0078d4,
        },
    ]
}

/// Build a core filled with demo conversations, with times relative to `now`.
pub fn demo_core(now_fn: Box<dyn Fn() -> Timestamp + Send>, utc_offset: i32) -> Result<Core> {
    let now = now_fn();
    let store = Store::open_in_memory()?;
    let mut core = Core::new(
        store,
        Box::new(DemoTransport::default()),
        now_fn,
        utc_offset,
    )?;
    for account in demo_accounts() {
        core.add_account(&account)?;
        for (pid, name, role) in [
            ("INBOX", "Inbox", FolderRole::Inbox),
            ("Sent", "Sent", FolderRole::Sent),
            ("Archive", "Archive", FolderRole::Archive),
        ] {
            core.upsert_folder(&account.id, pid, name, Some(role))?;
        }
    }

    let ada = "Ada Lovelace <ada@lovelace.example>";
    let grace = "Grace Hopper <grace@contoso.example>";
    let alan = "Alan Turing <alan@contoso.example>";
    let mum = "Mum <mum@family.example>";
    let me_gmail = "You <you@gmail.example>";
    let me_icloud = "You <you@icloud.example>";
    let me_work = "You <you@contoso.example>";

    let mails: Vec<(&str, FolderRole, bool, Mail<'_>)> = vec![
        // A one-to-one chat with quotes and a phone signature.
        (
            GMAIL,
            FolderRole::Inbox,
            true,
            Mail {
                id: "lunch-1@lovelace.example",
                from: ada,
                to: me_gmail,
                subject: "Lunch on Thursday?",
                ago: 5 * HOUR,
                body: "Hi! Are you free for lunch on Thursday? There's a new ramen place near the station.\n\nAda",
                ..Mail::default()
            },
        ),
        (
            GMAIL,
            FolderRole::Sent,
            true,
            Mail {
                id: "lunch-2@gmail.example",
                refs: &["lunch-1@lovelace.example"],
                from: me_gmail,
                to: ada,
                subject: "Re: Lunch on Thursday?",
                ago: 4 * HOUR,
                body: "Yes! 12:30 works for me.\n\nOn Mon, Ada Lovelace <ada@lovelace.example> wrote:\n> Hi! Are you free for lunch on Thursday? There's a new ramen place near the station.\n>\n> Ada",
                ..Mail::default()
            },
        ),
        (
            GMAIL,
            FolderRole::Inbox,
            false,
            Mail {
                id: "lunch-3@lovelace.example",
                refs: &["lunch-1@lovelace.example", "lunch-2@gmail.example"],
                from: ada,
                to: me_gmail,
                subject: "Re: Lunch on Thursday?",
                ago: 25 * 60,
                body: "Perfect, I'll book a table for two 🍜\n\nSent from my iPhone\n\nOn Mon, You <you@gmail.example> wrote:\n> Yes! 12:30 works for me.",
                ..Mail::default()
            },
        ),
        // A work group thread in Outlook style, with an attachment.
        (
            WORK,
            FolderRole::Inbox,
            true,
            Mail {
                id: "roadmap-1@contoso.example",
                from: grace,
                to: me_work,
                cc: alan,
                subject: "Q4 roadmap review",
                ago: DAY + 6 * HOUR,
                body: "Hi both,\n\nThe Q4 roadmap draft is attached. Could you review it before Wednesday's meeting?\n\nThanks,\nGrace\n\n--\nGrace Hopper\nHead of Engineering, Contoso",
                attachment: Some(("Q4-roadmap.pdf", "application/pdf")),
                ..Mail::default()
            },
        ),
        (
            WORK,
            FolderRole::Inbox,
            true,
            Mail {
                id: "roadmap-2@contoso.example",
                refs: &["roadmap-1@contoso.example"],
                from: alan,
                to: grace,
                cc: me_work,
                subject: "RE: Q4 roadmap review",
                ago: DAY + 3 * HOUR,
                body: "Looks good overall. I think the search work should move up to October.\n\nAlan\n\nFrom: Grace Hopper <grace@contoso.example>\nSent: Sunday 11:02\nTo: You <you@contoso.example>\nCc: Alan Turing <alan@contoso.example>\nSubject: Q4 roadmap review\n\nHi both,\nThe Q4 roadmap draft is attached.",
                ..Mail::default()
            },
        ),
        (
            WORK,
            FolderRole::Inbox,
            false,
            Mail {
                id: "roadmap-3@contoso.example",
                refs: &["roadmap-1@contoso.example", "roadmap-2@contoso.example"],
                from: grace,
                to: me_work,
                cc: alan,
                subject: "RE: Q4 roadmap review",
                ago: 2 * HOUR,
                body: "Agreed, moved it. Updated version is in the shared folder.\n\nThanks,\nGrace\n\n--\nGrace Hopper\nHead of Engineering, Contoso\n\n________________________________\nFrom: Alan Turing <alan@contoso.example>\nSent: Sunday 14:10\nTo: Grace Hopper <grace@contoso.example>\nSubject: RE: Q4 roadmap review\n\nLooks good overall.",
                ..Mail::default()
            },
        ),
        // Family, on iCloud.
        (
            ICLOUD,
            FolderRole::Inbox,
            false,
            Mail {
                id: "dinner-1@family.example",
                from: mum,
                to: me_icloud,
                subject: "Sunday dinner",
                ago: 3 * HOUR,
                body: "Are you coming for dinner on Sunday? Dad is making lasagne.\n\nLove, Mum\n\nSent from my iPad",
                ..Mail::default()
            },
        ),
        // A newsletter with tracking, shown as a card.
        (
            GMAIL,
            FolderRole::Inbox,
            false,
            Mail {
                id: "digest-881@news.example",
                from: "The Weekly Byte <digest@news.example>",
                to: me_gmail,
                subject: "This week: Rust 2026, new Linux desktops, and more",
                ago: 7 * HOUR,
                extra_headers: "List-Unsubscribe: <mailto:unsubscribe@news.example>\r\nList-Id: The Weekly Byte <digest.news.example>\r\n",
                body: "This week in tech: Rust 2026 lands, new Linux desktops, and more.",
                html: Some(
                    "<html><body><h1>The Weekly Byte</h1><p>This week in tech: <b>Rust 2026</b> lands, new Linux desktops, and more.</p><p><a href=\"https://news.example/rust?utm_source=newsletter&amp;utm_medium=email\">Read more</a></p><img src=\"https://us5.list-manage.com/track/open.php?u=1\" width=\"1\" height=\"1\"><img src=\"https://news.example/banner.png\"></body></html>",
                ),
                ..Mail::default()
            },
        ),
        // The same message delivered to two accounts, merged into one.
        (
            ICLOUD,
            FolderRole::Inbox,
            true,
            Mail {
                id: "trip-1@lovelace.example",
                from: ada,
                to: "You <you@icloud.example>, You <you@gmail.example>",
                subject: "Photos from the trip",
                ago: 2 * DAY,
                body: "Here are the photos from the weekend! The view from the top was amazing.",
                ..Mail::default()
            },
        ),
        (
            GMAIL,
            FolderRole::Inbox,
            true,
            Mail {
                id: "trip-1@lovelace.example",
                from: ada,
                to: "You <you@icloud.example>, You <you@gmail.example>",
                subject: "Photos from the trip",
                ago: 2 * DAY,
                body: "Here are the photos from the weekend! The view from the top was amazing.",
                ..Mail::default()
            },
        ),
        // A forward with a comment.
        (
            WORK,
            FolderRole::Inbox,
            true,
            Mail {
                id: "fwd-1@contoso.example",
                from: alan,
                to: me_work,
                subject: "Fwd: Office move",
                ago: 4 * DAY,
                body: "FYI, in case you missed it.\n\n---------- Forwarded message ---------\nFrom: Facilities <facilities@contoso.example>\nDate: Thu\nSubject: Office move\nTo: <all@contoso.example>\n\nWe move to the 4th floor on 1 November. Please pack your desks by Friday.",
                ..Mail::default()
            },
        ),
    ];

    for (n, (account, role, seen, mail)) in mails.iter().enumerate() {
        let folder = core.store().folder_by_role(account, *role)?;
        core.ingest(&Incoming {
            account_id: (*account).to_string(),
            provider_id: format!("demo-{n}"),
            provider_thread_id: None,
            raw: raw(mail, now),
            folder_ids: folder.into_iter().collect(),
            flags: Flags {
                seen: *seen,
                ..Flags::default()
            },
            received_at: now - mail.ago,
        })?;
    }
    core.refresh()?;
    Ok(core)
}
