use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use super::*;
use crate::demo::{GMAIL, WORK, demo_core};

// Monday 19 October 2026, 15:00 UTC.
const NOW: Timestamp = 1_792_422_000;

fn core_at(clock: &Arc<AtomicI64>) -> Core {
    let c = Arc::clone(clock);
    demo_core(Box::new(move || c.load(Ordering::SeqCst)), 0).expect("demo builds")
}

fn demo() -> (Core, Arc<AtomicI64>) {
    let clock = Arc::new(AtomicI64::new(NOW));
    (core_at(&clock), clock)
}

fn messages(conv: &Conversation) -> Vec<&Bubble> {
    conv.items
        .iter()
        .filter_map(|i| match i {
            ConversationItem::Message(b) => Some(b.as_ref()),
            ConversationItem::Day { .. } => None,
        })
        .collect()
}

fn thread_with_subject(core: &Core, subject: &str) -> ThreadRow {
    core.thread_list(&Mailbox::Inbox)
        .into_iter()
        .find(|r| r.subject == subject)
        .unwrap_or_else(|| panic!("no thread {subject}"))
}

#[test]
fn inbox_lists_threads_newest_first() {
    let (core, _) = demo();
    let rows = core.thread_list(&Mailbox::Inbox);
    let subjects: Vec<&str> = rows.iter().map(|r| r.subject.as_str()).collect();
    assert_eq!(
        subjects,
        vec![
            "Lunch on Thursday?",
            "Q4 roadmap review",
            "Sunday dinner",
            "This week: Rust 2026, new Linux desktops, and more",
            "Photos from the trip",
            "Fwd: Office move",
        ]
    );
    let lunch = &rows[0];
    assert_eq!(lunch.participants, "Ada Lovelace and Me");
    assert_eq!(lunch.message_count, 3);
    assert!(lunch.unread);
    assert_eq!(lunch.snippet, "Perfect, I'll book a table for two 🍜");
    assert_eq!(lunch.date_label, "14:35");
}

#[test]
fn copies_in_two_accounts_show_once_with_both_colours() {
    let (core, _) = demo();
    let trip = thread_with_subject(&core, "Photos from the trip");
    assert_eq!(trip.message_count, 1);
    assert_eq!(trip.account_colors, vec!["#34aadc", "#ea4335"]);
}

#[test]
fn account_inbox_filters_by_account() {
    let (core, _) = demo();
    let work: Vec<String> = core
        .thread_list(&Mailbox::AccountInbox(WORK.into()))
        .into_iter()
        .map(|r| r.subject)
        .collect();
    assert_eq!(work, vec!["Q4 roadmap review", "Fwd: Office move"]);
}

#[test]
fn conversation_shows_only_new_text_in_bubbles() {
    let (core, _) = demo();
    let lunch = thread_with_subject(&core, "Lunch on Thursday?");
    let conv = core.conversation(&lunch.thread_id).expect("conversation");
    assert!(!conv.is_group);
    assert_eq!(conv.reply_label, "To Ada Lovelace");
    let bubbles = messages(&conv);
    assert_eq!(bubbles.len(), 3);
    assert_eq!(bubbles[1].text, "Yes! 12:30 works for me.");
    assert!(bubbles[1].is_me);
    assert!(bubbles[1].quoted.is_some());
    assert_eq!(bubbles[2].text, "Perfect, I'll book a table for two 🍜");
    assert_eq!(bubbles[2].signature.as_deref(), Some("Sent from my iPhone"));
    assert!(matches!(conv.items[0], ConversationItem::Day { ref label } if label == "Today"));
}

#[test]
fn group_conversation_strips_outlook_quotes_and_learned_signatures() {
    let (core, _) = demo();
    let roadmap = thread_with_subject(&core, "Q4 roadmap review");
    let conv = core.conversation(&roadmap.thread_id).expect("conversation");
    assert!(conv.is_group);
    assert_eq!(conv.reply_label, "To Grace Hopper and Alan Turing");
    let bubbles = messages(&conv);
    assert_eq!(
        bubbles[1].text,
        "Looks good overall. I think the search work should move up to October.\n\nAlan"
    );
    assert_eq!(
        bubbles[2].text,
        "Agreed, moved it. Updated version is in the shared folder.\n\nThanks,\nGrace"
    );
    assert!(bubbles[0].show_sender);
    assert_eq!(bubbles[0].attachments.len(), 1);
    assert_eq!(bubbles[0].attachments[0].filename, "Q4-roadmap.pdf");
}

#[test]
fn newsletters_render_as_cards_without_trackers() {
    let (core, _) = demo();
    let digest = thread_with_subject(&core, "This week: Rust 2026, new Linux desktops, and more");
    assert!(digest.is_bulk);
    let conv = core.conversation(&digest.thread_id).expect("conversation");
    let bubble = messages(&conv)[0];
    assert!(bubble.is_card);
    assert!(bubble.has_html);
    let id = bubble.message_id;
    let html = core
        .store()
        .body(id)
        .ok()
        .flatten()
        .and_then(|b| b.html)
        .expect("html");
    assert!(!html.contains("list-manage"));
    assert!(!html.contains("banner.png"));
    assert!(!html.contains("utm_"));
}

#[test]
fn forwards_keep_the_comment_and_flag_the_forward() {
    let (core, _) = demo();
    let fwd = thread_with_subject(&core, "Fwd: Office move");
    let conv = core.conversation(&fwd.thread_id).expect("conversation");
    let bubble = messages(&conv)[0];
    assert_eq!(bubble.text, "FYI, in case you missed it.");
    assert!(bubble.forwarded);
}

#[test]
fn people_view_groups_by_participants() {
    let (core, _) = demo();
    let people = core.people_list(&Mailbox::Inbox);
    let names: Vec<&str> = people.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Ada Lovelace",
            "Alan Turing and Grace Hopper",
            "Mum",
            "The Weekly Byte",
            "Alan Turing"
        ]
    );
    let ada = &people[0];
    assert_eq!(
        ada.thread_count, 2,
        "lunch and trip photos are both with Ada"
    );
    let conv = core.person_conversation(&ada.key).expect("conversation");
    let bubbles = messages(&conv);
    assert_eq!(bubbles.len(), 4);
    assert_eq!(
        bubbles[1].subject_change.as_deref(),
        Some("Lunch on Thursday?")
    );
    assert!(people[1].is_group);
}

#[test]
fn reply_goes_out_after_the_undo_window() {
    let (mut core, clock) = demo();
    let lunch = thread_with_subject(&core, "Lunch on Thursday?");
    let pending = core
        .send_reply(&lunch.thread_id, "See you there!")
        .expect("queued");
    assert_eq!(pending.seconds_left, UNDO_SEND_SECONDS);
    let conv = core.conversation(&lunch.thread_id).expect("conversation");
    let last = *messages(&conv).last().expect("bubble");
    assert_eq!(last.text, "See you there!");
    assert_eq!(last.delivery, DeliveryState::Sending);

    assert_eq!(
        core.process_outbox().expect("processed"),
        0,
        "nothing sends inside the window"
    );
    clock.fetch_add(UNDO_SEND_SECONDS, Ordering::SeqCst);
    assert_eq!(core.process_outbox().expect("processed"), 1);

    let conv = core.conversation(&lunch.thread_id).expect("conversation");
    let bubbles = messages(&conv);
    assert_eq!(bubbles.len(), 4);
    let sent = bubbles[3];
    assert_eq!(sent.text, "See you there!");
    assert_eq!(sent.delivery, DeliveryState::Sent);
    assert!(
        sent.quoted.is_some(),
        "the outgoing mail quotes the previous message"
    );
    assert!(core.pending_sends().is_empty());
}

#[test]
fn group_reply_goes_to_everyone_from_the_work_account() {
    let (mut core, clock) = demo();
    let roadmap = thread_with_subject(&core, "Q4 roadmap review");
    core.send_reply(&roadmap.thread_id, "Thanks both!")
        .expect("queued");
    clock.fetch_add(UNDO_SEND_SECONDS, Ordering::SeqCst);
    core.process_outbox().expect("processed");
    let sent = core
        .store()
        .all_headers()
        .expect("headers")
        .into_iter()
        .find(|h| h.snippet == "Thanks both!")
        .expect("sent copy stored");
    assert_eq!(sent.account_id, WORK);
    assert_eq!(sent.from.email, "you@contoso.example");
    let mut recipients: Vec<String> = sent
        .to
        .iter()
        .chain(sent.cc.iter())
        .map(|a| a.email.clone())
        .collect();
    recipients.sort();
    assert_eq!(
        recipients,
        vec!["alan@contoso.example", "grace@contoso.example"]
    );
    assert_eq!(sent.in_reply_to, vec!["roadmap-3@contoso.example"]);
    assert_eq!(sent.subject, "Re: Q4 roadmap review");
}

#[test]
fn undo_send_returns_the_text_and_cancels() {
    let (mut core, clock) = demo();
    let lunch = thread_with_subject(&core, "Lunch on Thursday?");
    let pending = core
        .send_reply(&lunch.thread_id, "Oops, wrong chat")
        .expect("queued");
    clock.fetch_add(4, Ordering::SeqCst);
    assert_eq!(
        core.undo_send(pending.outbox_id).expect("undo").as_deref(),
        Some("Oops, wrong chat")
    );
    clock.fetch_add(UNDO_SEND_SECONDS, Ordering::SeqCst);
    assert_eq!(core.process_outbox().expect("processed"), 0);
    let conv = core.conversation(&lunch.thread_id).expect("conversation");
    assert_eq!(messages(&conv).len(), 3);
}

#[test]
fn undo_is_too_late_once_the_window_has_passed() {
    let (mut core, clock) = demo();
    let lunch = thread_with_subject(&core, "Lunch on Thursday?");
    let pending = core.send_reply(&lunch.thread_id, "Hi").expect("queued");
    clock.fetch_add(UNDO_SEND_SECONDS, Ordering::SeqCst);
    assert_eq!(core.undo_send(pending.outbox_id).expect("undo"), None);
}

#[test]
fn done_archives_and_undo_brings_it_back() {
    let (mut core, _) = demo();
    let mum = thread_with_subject(&core, "Sunday dinner");
    core.mark_done(&mum.thread_id).expect("done");
    assert!(
        core.thread_list(&Mailbox::Inbox)
            .iter()
            .all(|r| r.subject != "Sunday dinner")
    );
    assert!(
        core.thread_list(&Mailbox::Done)
            .iter()
            .any(|r| r.subject == "Sunday dinner")
    );
    core.undo_done(&mum.thread_id).expect("undo");
    assert!(
        core.thread_list(&Mailbox::Inbox)
            .iter()
            .any(|r| r.subject == "Sunday dinner")
    );
}

#[test]
fn pinned_threads_float_to_the_top() {
    let (mut core, _) = demo();
    let fwd = thread_with_subject(&core, "Fwd: Office move");
    core.toggle_pin(&fwd.thread_id).expect("pin");
    let rows = core.thread_list(&Mailbox::Inbox);
    assert_eq!(rows[0].subject, "Fwd: Office move");
    assert!(rows[0].pinned);
    assert_eq!(core.thread_list(&Mailbox::Pinned).len(), 1);
    core.toggle_pin(&fwd.thread_id).expect("unpin");
    assert!(core.thread_list(&Mailbox::Pinned).is_empty());
}

#[test]
fn opening_marks_read_and_queues_the_remote_change() {
    let (mut core, _) = demo();
    let lunch = thread_with_subject(&core, "Lunch on Thursday?");
    assert!(lunch.unread);
    let before = core.sidebar().items[0].unread_count;
    core.mark_read(&lunch.thread_id).expect("read");
    assert!(!thread_with_subject(&core, "Lunch on Thursday?").unread);
    assert_eq!(core.sidebar().items[0].unread_count, before - 1);
    let queued = core.store().pending_outbox().expect("outbox");
    assert!(
        queued
            .iter()
            .any(|i| i.account_id == GMAIL && i.payload.contains("mark_read"))
    );
}

#[test]
fn sidebar_lists_mailboxes_and_accounts() {
    let (core, _) = demo();
    let sidebar = core.sidebar();
    let labels: Vec<&str> = sidebar.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "Inbox",
            "Pinned",
            "Sent",
            "Done",
            "you@icloud.example",
            "you@gmail.example",
            "you@contoso.example"
        ]
    );
    assert_eq!(sidebar.accounts[2].provider, "Microsoft 365");
    let json = serde_json::to_string(&sidebar).expect("serialises");
    assert!(
        json.contains(r#""mailbox":{"kind":"account_inbox","account":"demo-icloud"}"#),
        "{json}"
    );
}

#[test]
fn view_models_serialise_for_the_ui() {
    let (core, _) = demo();
    let lunch = thread_with_subject(&core, "Lunch on Thursday?");
    let conv = core.conversation(&lunch.thread_id).expect("conversation");
    let json = serde_json::to_value(&conv).expect("serialises");
    assert_eq!(json["items"][0]["type"], "day");
    assert_eq!(json["items"][1]["type"], "message");
    assert_eq!(json["items"][1]["delivery"], "received");
}
