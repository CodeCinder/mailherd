//! MailHerd's core for the Apple apps, through UniFFI.
//!
//! [`MailHerdApp`] wraps the [`mh_ui::Controller`] that the Linux app also
//! uses, with the same methods: view models come back as JSON strings and
//! actions go in as calls. Swift calls it from the main thread.

use std::sync::{Arc, Mutex, MutexGuard};

use mh_ui::Controller;

uniffi::setup_scaffolding!();

#[derive(uniffi::Object)]
pub struct MailHerdApp {
    inner: Mutex<Controller>,
}

impl MailHerdApp {
    fn c(&self) -> MutexGuard<'_, Controller> {
        // A panic mid-call can't leave the controller half-changed in a way
        // that matters more than stopping the app would, so carry on.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl MailHerdApp {
    /// Open the real accounts (or the demo with `MAILHERD_DEMO=1`).
    #[uniffi::constructor]
    pub fn launch(utc_offset_seconds: i32) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Controller::launch(utc_offset_seconds)),
        })
    }

    /// Changes whenever anything on screen should be re-read.
    pub fn revision(&self) -> u64 {
        self.c().revision()
    }

    pub fn is_demo(&self) -> bool {
        self.c().is_demo()
    }

    pub fn show_welcome(&self) -> bool {
        self.c().show_welcome()
    }

    pub fn mode(&self) -> String {
        self.c().mode().to_string()
    }

    pub fn mailbox_json(&self) -> String {
        self.c().mailbox_json()
    }

    pub fn sidebar_json(&self) -> String {
        self.c().sidebar_json()
    }

    pub fn list_json(&self) -> String {
        self.c().list_json()
    }

    pub fn conversation_json(&self, id: String) -> String {
        self.c().conversation_json(&id)
    }

    pub fn message_html(&self, id: i64) -> String {
        self.c().message_html(id)
    }

    pub fn pending_json(&self) -> String {
        self.c().pending_json()
    }

    pub fn last_error(&self) -> String {
        self.c().last_error().to_string()
    }

    pub fn startup_error(&self) -> String {
        self.c().startup_error().to_string()
    }

    pub fn can_start_fresh(&self) -> bool {
        self.c().can_start_fresh()
    }

    pub fn sign_in_json(&self) -> String {
        self.c().sign_in_json()
    }

    pub fn set_utc_offset(&self, seconds: i32) {
        self.c().set_utc_offset(seconds);
    }

    pub fn select_mailbox(&self, mailbox_json: String) {
        self.c().select_mailbox(&mailbox_json);
    }

    pub fn set_list_mode(&self, mode: String) {
        self.c().set_list_mode(&mode);
    }

    pub fn open_conversation(&self, id: String) {
        self.c().open_conversation(&id);
    }

    pub fn toggle_pin(&self, id: String) {
        self.c().toggle_pin(&id);
    }

    pub fn mark_done(&self, id: String) {
        self.c().mark_done(&id);
    }

    pub fn undo_done(&self, id: String) {
        self.c().undo_done(&id);
    }

    pub fn mark_unread(&self, id: String) {
        self.c().mark_unread(&id);
    }

    pub fn send_reply(&self, id: String, text: String) -> String {
        self.c().send_reply(&id, &text)
    }

    pub fn undo_send(&self, outbox_id: i64) -> String {
        self.c().undo_send(outbox_id)
    }

    pub fn tick(&self) -> bool {
        self.c().tick()
    }

    pub fn start_sign_in(&self, provider: String, email: String, password: String) {
        self.c().start_sign_in(&provider, &email, &password);
    }

    pub fn dismiss_sign_in(&self) {
        self.c().dismiss_sign_in();
    }

    pub fn remove_account(&self, id: String) {
        self.c().remove_account(&id);
    }

    pub fn sync_now(&self) {
        self.c().sync_now();
    }

    pub fn use_demo(&self) {
        self.c().use_demo();
    }

    pub fn open_accounts(&self) {
        self.c().open_accounts();
    }

    pub fn start_fresh(&self) {
        self.c().start_fresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_swift_facade_drives_the_controller() {
        let app = MailHerdApp {
            inner: Mutex::new(Controller::new(
                Box::new(|_| Err(std::io::Error::other("no accounts in tests").into())),
                None,
                0,
            )),
        };
        app.use_demo();
        assert!(app.is_demo());
        let before = app.revision();
        app.set_list_mode("people".into());
        assert_eq!(app.mode(), "people");
        assert!(app.revision() > before);
        assert!(app.list_json().starts_with('['));
    }
}
