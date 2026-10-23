use super::*;
use std::sync::Arc;

use mh_auth::{MemoryStore, Vault};
use mh_sync::{Live, Timing};
use serde_json::Value;

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("valid JSON")
}

fn demo() -> Controller {
    let mut c = Controller::new(Box::new(|_| Err(no_home())), None, 0);
    c.use_demo();
    c
}

fn first_thread(c: &Controller) -> Value {
    parse(&c.list_json())[0].clone()
}

/// A data folder of its own, removed afterwards.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mh-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Real accounts in `dir`, with `vault` standing in for the system keychain.
fn real(dir: &TempDir, vault: Vault) -> Controller {
    let path = dir.0.clone();
    let opener: Opener = Box::new(move |offset| {
        Session::open_with(
            &path,
            Arc::new(Live::new(vault.clone())),
            Timing::default(),
            offset,
        )
    });
    let mut c = Controller::new(opener, Some(dir.0.clone()), 0);
    c.open_accounts();
    c
}

fn memory_vault() -> Vault {
    Vault::new(Arc::new(MemoryStore::default()))
}

#[test]
fn the_demo_shows_mail_without_the_welcome_screen() {
    let c = demo();
    assert!(c.is_demo());
    assert!(!c.show_welcome());
    let sidebar = parse(&c.sidebar_json());
    assert_eq!(sidebar["accounts"].as_array().map(Vec::len), Some(3));
    assert!(!parse(&c.list_json()).as_array().expect("rows").is_empty());
    assert_eq!(c.mailbox_json(), r#"{"kind":"inbox"}"#);
}

#[test]
fn switching_lists_and_mailboxes() {
    let mut c = demo();
    let before = c.revision();
    c.set_list_mode("people");
    assert_eq!(c.mode(), "people");
    assert!(c.revision() > before);
    assert!(first_thread(&c)["key"].is_string(), "People rows");

    c.set_list_mode("nonsense");
    assert_eq!(c.mode(), "threads");

    c.select_mailbox(r#"{"kind":"done"}"#);
    assert_eq!(c.mailbox_json(), r#"{"kind":"done"}"#);
    c.select_mailbox("not json");
    assert_eq!(c.mailbox_json(), r#"{"kind":"inbox"}"#);
}

#[test]
fn opening_a_conversation_marks_it_read() {
    let mut c = demo();
    let row = parse(&c.list_json())
        .as_array()
        .expect("rows")
        .iter()
        .find(|r| r["unread"] == true)
        .cloned()
        .expect("an unread thread");
    let id = row["thread_id"].as_str().expect("id").to_string();
    assert_ne!(c.conversation_json(&id), "null");
    c.open_conversation(&id);
    let after = parse(&c.list_json());
    let row = after
        .as_array()
        .expect("rows")
        .iter()
        .find(|r| r["thread_id"] == id.as_str())
        .expect("still listed");
    assert_eq!(row["unread"], false);
}

#[test]
fn a_reply_can_be_undone() {
    let mut c = demo();
    let id = first_thread(&c)["thread_id"]
        .as_str()
        .expect("id")
        .to_string();
    let pending = parse(&c.send_reply(&id, "On my way"));
    let outbox = pending["outbox_id"].as_i64().expect("outbox id");
    assert_eq!(parse(&c.pending_json()).as_array().map(Vec::len), Some(1));
    assert_eq!(c.undo_send(outbox), "On my way");
    assert_eq!(c.pending_json(), "[]");
    assert_eq!(c.last_error(), "");
}

#[test]
fn bad_ids_report_errors_without_crashing() {
    let mut c = demo();
    assert_eq!(c.send_reply("no-such-thread", "hi"), "");
    assert!(!c.last_error().is_empty());
    assert_eq!(c.conversation_json("no-such-thread"), "null");
    assert_eq!(c.message_html(-1), "");
}

#[test]
fn a_new_install_shows_the_welcome_screen() {
    let dir = TempDir::new("new");
    let c = real(&dir, memory_vault());
    assert!(!c.is_demo());
    assert_eq!(c.startup_error(), "");
    assert!(c.show_welcome());
    assert_eq!(c.sign_in_json(), "null");
    assert_eq!(parse(&c.sidebar_json())["accounts"], serde_json::json!([]));
}

#[test]
fn leaving_the_demo_opens_the_real_accounts() {
    let dir = TempDir::new("leave");
    let mut c = real(&dir, memory_vault());
    c.use_demo();
    assert!(c.is_demo());
    c.open_accounts();
    assert!(!c.is_demo());
    assert!(c.show_welcome());
}

#[test]
fn a_lost_key_offers_start_fresh() {
    let dir = TempDir::new("lost");
    drop(real(&dir, memory_vault()));

    // The keychain forgot the key: a different, empty vault.
    let mut c = real(&dir, memory_vault());
    assert!(!c.startup_error().is_empty());
    assert!(c.can_start_fresh());
    assert!(c.show_welcome());

    c.start_fresh();
    assert_eq!(c.startup_error(), "");
    assert!(!c.can_start_fresh());
    assert!(c.show_welcome(), "an empty mailbox, ready for accounts");
}

#[test]
fn other_startup_problems_do_not_offer_start_fresh() {
    let mut c = Controller::new(Box::new(|_| Err(no_home())), None, 0);
    c.open_accounts();
    assert!(c.startup_error().contains("home folder"));
    assert!(!c.can_start_fresh());
    assert!(c.show_welcome());
    assert!(!c.tick());
}
