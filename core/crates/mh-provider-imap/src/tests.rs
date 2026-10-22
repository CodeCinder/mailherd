//! Tests against a real IMAP server (Dovecot) started for each test, plus a
//! fake SMTP server. Skipped with a notice when Dovecot isn't installed.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

struct Dovecot {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Dovecot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(0)
}

fn current_uid() -> u32 {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(65534)
}

fn is_root() -> bool {
    current_uid() == 0
}

fn allow_everyone(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o777));
}

fn dovecot_binary() -> Option<PathBuf> {
    ["/usr/sbin/dovecot", "/usr/bin/dovecot"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

fn start_dovecot(name: &str) -> Option<Dovecot> {
    let Some(bin) = dovecot_binary() else {
        eprintln!("skipping IMAP test: dovecot is not installed");
        return None;
    };
    // Unix socket paths are limited to about 100 bytes, so keep this short.
    let dir = PathBuf::from("/tmp").join(format!("mh-imap-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("run")).ok()?;
    std::fs::create_dir_all(dir.join("state")).ok()?;
    let port = free_port();
    std::fs::write(dir.join("passwd"), "tester:{PLAIN}secret::::::\n").ok()?;
    // Dovecot won't store mail as root, so mail belongs to an ordinary user.
    let uid = if is_root() { 65534 } else { current_uid() };
    allow_everyone(&dir);
    let d = dir.display();
    let conf = format!(
        r#"protocols = imap
listen = 127.0.0.1
base_dir = {d}/run
state_dir = {d}/state
log_path = {d}/dovecot.log
ssl = no
disable_plaintext_auth = no
auth_mechanisms = plain login
mail_location = maildir:{d}/mail/%u
first_valid_uid = 1
passdb {{
  driver = passwd-file
  args = {d}/passwd
}}
userdb {{
  driver = static
  args = uid={uid} gid={uid} home={d}/home/%u
}}
service imap-login {{
  inet_listener imap {{
    port = {port}
  }}
  inet_listener imaps {{
    port = 0
  }}
}}
namespace inbox {{
  inbox = yes
  mailbox "Sent Messages" {{
    special_use = \Sent
    auto = create
  }}
  mailbox Archive {{
    special_use = \Archive
    auto = create
  }}
  mailbox "Deleted Messages" {{
    special_use = \Trash
    auto = create
  }}
}}
"#
    );
    std::fs::write(dir.join("dovecot.conf"), conf).ok()?;
    let child = Command::new(bin)
        .args(["-F", "-c"])
        .arg(dir.join("dovecot.conf"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let server = Dovecot { child, port, dir };
    let deadline = Instant::now() + Duration::from_secs(10);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        if Instant::now() > deadline {
            let log = std::fs::read_to_string(server.dir.join("dovecot.log")).unwrap_or_default();
            panic!("dovecot didn't start:\n{log}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Some(server)
}

fn config(server: &Dovecot, smtp_port: u16) -> ImapConfig {
    ImapConfig {
        imap_host: "127.0.0.1".into(),
        imap_port: server.port,
        smtp_host: "127.0.0.1".into(),
        smtp_port,
        tls: false,
        username: "tester".into(),
        password: "secret".into(),
    }
}

/// A direct IMAP session for putting test mail on the server.
fn admin(server: &Dovecot) -> Session {
    let client = ClientBuilder::new("127.0.0.1", server.port)
        .mode(ConnectionMode::Plaintext)
        .connect()
        .expect("connect");
    client
        .login("tester", "secret")
        .map_err(|(e, _)| e)
        .expect("login")
}

fn mail(id: &str, subject: &str) -> Vec<u8> {
    format!(
        "Message-ID: <{id}>\r\nFrom: Mum <mum@family.example>\r\nTo: tester@icloud.example\r\nSubject: {subject}\r\nDate: {}\r\n\r\nHello from the test.\r\n",
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc2822)
            .unwrap_or_default()
    )
    .into_bytes()
}

fn run(imap: &mut Imap, cursor: Option<&str>) -> (String, Vec<SyncEvent>) {
    let mut events = Vec::new();
    let cursor = imap.sync(cursor, &mut |e| events.push(e)).expect("synced");
    (cursor, events)
}

fn message_ids(events: &[SyncEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::Message { provider_id, .. } => Some(provider_id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn uid_sets_compress_ranges() {
    let set: BTreeSet<u32> = [1, 2, 3, 5, 7, 8].into_iter().collect();
    assert_eq!(uid_set(&set), "1:3,5,7:8");
    assert_eq!(
        parse_provider_id("7:42:Sent Messages"),
        Some((7, 42, "Sent Messages"))
    );
    assert_eq!(parse_provider_id("7:42:a:b"), Some((7, 42, "a:b")));
}

#[test]
fn first_and_incremental_syncs() {
    let Some(server) = start_dovecot("sync") else {
        return;
    };
    let mut a = admin(&server);
    a.append("INBOX", &mail("one@x", "First"))
        .finish()
        .expect("append");
    a.append("INBOX", &mail("two@x", "Second"))
        .flag(Flag::Seen)
        .finish()
        .expect("append");
    a.append("Sent Messages", &mail("sent@x", "Mine"))
        .flag(Flag::Seen)
        .finish()
        .expect("append");

    let mut imap = Imap::new("ic".into(), config(&server, 0));
    let (cursor, events) = run(&mut imap, None);

    let folders = events.iter().find_map(|e| match e {
        SyncEvent::Folders { folders, .. } => Some(folders.clone()),
        _ => None,
    });
    let roles: Vec<(String, Option<FolderRole>)> = folders
        .expect("folders")
        .into_iter()
        .map(|f| (f.provider_id, f.role))
        .collect();
    assert!(roles.contains(&("INBOX".into(), Some(FolderRole::Inbox))));
    assert!(roles.contains(&("Sent Messages".into(), Some(FolderRole::Sent))));
    assert!(roles.contains(&("Archive".into(), Some(FolderRole::Archive))));

    let ids = message_ids(&events);
    assert_eq!(ids.len(), 3, "{ids:?}");
    let seen: Vec<bool> = events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::Message { folders, flags, .. } if folders == &vec!["INBOX".to_string()] => {
                Some(flags.seen)
            }
            _ => None,
        })
        .collect();
    assert_eq!(seen.len(), 2);
    assert!(seen.contains(&true) && seen.contains(&false));

    // Nothing changed: no downloads, just flag refreshes.
    let (cursor, events) = run(&mut imap, Some(&cursor));
    assert!(message_ids(&events).is_empty());

    // Mark one read, delete another, add a new one.
    a.select("INBOX").expect("select");
    let uids: Vec<u32> = {
        let mut u: Vec<u32> = a.uid_search("ALL").expect("search").into_iter().collect();
        u.sort_unstable();
        u
    };
    a.uid_store(uids[0].to_string(), "+FLAGS (\\Seen)")
        .expect("store");
    a.uid_store(uids[1].to_string(), "+FLAGS (\\Deleted)")
        .expect("store");
    a.expunge().expect("expunge");
    a.append("INBOX", &mail("three@x", "Third"))
        .finish()
        .expect("append");

    let (_, events) = run(&mut imap, Some(&cursor));
    let new = message_ids(&events);
    assert_eq!(new.len(), 1);
    assert!(events.iter().any(
        |e| matches!(e, SyncEvent::State { provider_id, flags: Some(f), .. }
        if parse_provider_id(provider_id).map(|p| p.1) == Some(uids[0]) && f.seen)
    ));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SyncEvent::Removed { provider_id, .. }
        if parse_provider_id(provider_id).map(|p| p.1) == Some(uids[1])))
    );
}

#[test]
fn actions_change_the_mailbox() {
    let Some(server) = start_dovecot("actions") else {
        return;
    };
    let mut a = admin(&server);
    a.append("INBOX", &mail("one@x", "First"))
        .finish()
        .expect("append");
    let mut imap = Imap::new("ic".into(), config(&server, 0));
    let (_, events) = run(&mut imap, None);
    let id = message_ids(&events).remove(0);

    imap.apply(&RemoteAction::MarkRead {
        provider_ids: vec![id.clone()],
    })
    .expect("read");
    a.select("INBOX").expect("select");
    let flags = a.uid_fetch("1:*", "FLAGS").expect("fetch");
    assert!(flags.iter().all(|f| f.flags().contains(&Flag::Seen)));

    imap.apply(&RemoteAction::Archive {
        provider_ids: vec![id],
    })
    .expect("archived");
    assert_eq!(a.select("INBOX").expect("select").exists, 0);
    assert_eq!(a.select("Archive").expect("select").exists, 1);
}

/// Recipients and message text of one SMTP delivery.
type Delivered = Arc<Mutex<Option<(Vec<String>, String)>>>;

/// Accept one SMTP session and keep the message it delivers.
fn fake_smtp() -> (u16, Delivered) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let got = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&got);
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut out = stream.try_clone().expect("clone");
        let mut lines = BufReader::new(stream);
        let _ = write!(out, "220 fake ESMTP\r\n");
        let mut rcpt = Vec::new();
        let mut data = String::new();
        loop {
            let mut line = String::new();
            if lines.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let upper = line.to_ascii_uppercase();
            if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                let _ = write!(out, "250-fake\r\n250 8BITMIME\r\n");
            } else if upper.starts_with("RCPT TO:") {
                rcpt.push(line[8..].trim().to_string());
                let _ = write!(out, "250 ok\r\n");
            } else if upper.starts_with("DATA") {
                let _ = write!(out, "354 go\r\n");
                loop {
                    let mut l = String::new();
                    if lines.read_line(&mut l).unwrap_or(0) == 0 || l == ".\r\n" {
                        break;
                    }
                    data.push_str(&l);
                }
                *slot.lock().expect("lock") = Some((rcpt.clone(), data.clone()));
                let _ = write!(out, "250 queued\r\n");
            } else if upper.starts_with("QUIT") {
                let _ = write!(out, "221 bye\r\n");
                return;
            } else {
                let _ = write!(out, "250 ok\r\n");
            }
        }
    });
    (port, got)
}

#[test]
fn sending_delivers_and_files_a_copy_in_sent() {
    let Some(server) = start_dovecot("send") else {
        return;
    };
    let (smtp_port, got) = fake_smtp();
    let mut imap = Imap::new("ic".into(), config(&server, smtp_port));
    let raw = b"Message-ID: <out@icloud.example>\r\nFrom: Me <tester@icloud.example>\r\nTo: Mum <mum@family.example>\r\nCc: dad@family.example\r\nSubject: Sunday\r\n\r\nSee you there!\r\n";
    imap.send(raw).expect("sent");

    let (rcpt, data) = got.lock().expect("lock").clone().expect("delivered");
    assert_eq!(rcpt, vec!["<mum@family.example>", "<dad@family.example>"]);
    assert!(data.contains("See you there!"));
    let mut a = admin(&server);
    assert_eq!(a.select("Sent Messages").expect("select").exists, 1);
}

#[test]
fn a_wrong_password_asks_for_sign_in() {
    let Some(server) = start_dovecot("auth") else {
        return;
    };
    let mut cfg = config(&server, 0);
    cfg.password = "wrong".into();
    assert_eq!(Imap::check_login(&cfg), Err(ProviderError::NeedsSignIn));
    assert_eq!(Imap::check_login(&config(&server, 0)), Ok(()));
}
