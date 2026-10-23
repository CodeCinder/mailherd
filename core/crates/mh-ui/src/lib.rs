//! The controller every MailHerd app drives.
//!
//! A [`Controller`] owns the [`Session`] and the little state a window needs
//! (which mailbox is showing, Threads or People). Apps read view models as
//! JSON strings, call actions, and re-read whenever [`Controller::revision`]
//! changes. The Linux app reaches it through cxx-qt and the Apple apps
//! through UniFFI, so both behave the same.

use std::path::PathBuf;

use mh_app::Core;
use mh_app::view::Mailbox;
use mh_sync::{NewAccount, Session, SessionError};

/// Opens the real accounts, given the UTC offset in seconds.
pub type Opener = Box<dyn Fn(i32) -> Result<Session, SessionError> + Send>;

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

fn no_home() -> SessionError {
    std::io::Error::other("there is no home folder").into()
}

pub struct Controller {
    session: Option<Session>,
    open_real: Opener,
    /// Where the real mail is stored, for Start fresh. `None` when nothing is saved.
    data_dir: Option<PathBuf>,
    error: String,
    startup_error: String,
    can_start_fresh: bool,
    utc_offset: i32,
    mode: String,
    mailbox: Mailbox,
    revision: u64,
}

impl Controller {
    /// The app as it starts: the real accounts in the user's data folder.
    /// `MAILHERD_DEMO=1` opens the demo instead (used for screenshots) and
    /// `MAILHERD_EPHEMERAL=1` keeps everything in memory, for trying sign-in.
    pub fn launch(utc_offset: i32) -> Self {
        let ephemeral = flag("MAILHERD_EPHEMERAL");
        let data_dir = if ephemeral {
            None
        } else {
            mh_sync::default_data_dir()
        };
        let dir = data_dir.clone();
        let open_real: Opener = Box::new(move |offset| {
            if ephemeral {
                return Session::ephemeral(offset);
            }
            let dir = dir.as_deref().ok_or_else(no_home)?;
            Session::open(dir, offset)
        });
        let mut this = Self::new(open_real, data_dir, utc_offset);
        if flag("MAILHERD_DEMO") {
            this.use_demo();
        } else {
            this.open_accounts();
        }
        this
    }

    /// A controller with nothing open yet. Call [`Controller::open_accounts`]
    /// or [`Controller::use_demo`] next.
    pub fn new(open_real: Opener, data_dir: Option<PathBuf>, utc_offset: i32) -> Self {
        Self {
            session: None,
            open_real,
            data_dir,
            error: String::new(),
            startup_error: String::new(),
            can_start_fresh: false,
            utc_offset,
            mode: "threads".into(),
            mailbox: Mailbox::Inbox,
            revision: 0,
        }
    }

    /// Changes whenever anything on screen should be re-read.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    fn core(&self) -> Option<&Core> {
        self.session.as_ref().map(Session::core)
    }

    /// Run an action on the core, record any error, and refresh the views.
    fn act(&mut self, f: impl FnOnce(&mut Core) -> mh_app::Result<()>) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.error = f(session.core_mut())
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        self.bump();
    }

    /// Swap in a new session (or the error that stopped it opening).
    fn switch_session(&mut self, opened: Result<Session, SessionError>) {
        // Stop the old session's sync before the new one opens the database.
        self.session = None;
        match opened {
            Ok(mut session) => {
                session.core_mut().set_utc_offset(self.utc_offset);
                self.startup_error.clear();
                self.can_start_fresh = false;
                self.session = Some(session);
            }
            Err(e) => {
                self.startup_error = e.to_string();
                self.can_start_fresh = e.can_start_fresh();
            }
        }
        self.mailbox = Mailbox::Inbox;
        self.bump();
    }

    // ---- what to show ----

    pub fn is_demo(&self) -> bool {
        self.session.as_ref().is_some_and(Session::is_demo)
    }

    /// The welcome screen: no accounts yet, or MailHerd couldn't start.
    pub fn show_welcome(&self) -> bool {
        if self.is_demo() {
            return false;
        }
        !self.startup_error.is_empty() || self.core().is_none_or(|c| c.accounts().is_empty())
    }

    /// "threads" or "people".
    pub fn mode(&self) -> &str {
        &self.mode
    }

    /// The mailbox showing, as the JSON the sidebar items carry.
    pub fn mailbox_json(&self) -> String {
        json(&self.mailbox)
    }

    pub fn sidebar_json(&self) -> String {
        match self.core() {
            Some(core) => json(&core.sidebar()),
            None => "null".into(),
        }
    }

    pub fn list_json(&self) -> String {
        let Some(core) = self.core() else {
            return "[]".into();
        };
        if self.mode == "people" {
            json(&core.people_list(&self.mailbox))
        } else {
            json(&core.thread_list(&self.mailbox))
        }
    }

    /// A thread by id, or a person or group from the People list ("people:<key>").
    pub fn conversation_json(&self, id: &str) -> String {
        let Some(core) = self.core() else {
            return "null".into();
        };
        let conversation = match id.strip_prefix("people:") {
            Some(key) => core.person_conversation(key),
            None => core.conversation(id),
        };
        json(&conversation)
    }

    /// A message's sanitised HTML, for "show original". Empty if there is none.
    pub fn message_html(&self, id: i64) -> String {
        self.core()
            .and_then(|core| core.message_html(id))
            .unwrap_or_default()
    }

    pub fn pending_json(&self) -> String {
        match self.core() {
            Some(core) => json(&core.pending_sends()),
            None => "[]".into(),
        }
    }

    pub fn last_error(&self) -> &str {
        &self.error
    }

    /// Why MailHerd couldn't open its mail, or empty.
    pub fn startup_error(&self) -> &str {
        &self.startup_error
    }

    /// The saved mail can't be unlocked, and [`Controller::start_fresh`] is the way out.
    pub fn can_start_fresh(&self) -> bool {
        self.can_start_fresh
    }

    /// The account being added, as JSON, or "null".
    pub fn sign_in_json(&self) -> String {
        match self.session.as_ref().and_then(Session::sign_in_state) {
            Some(state) => json(state),
            None => "null".into(),
        }
    }

    // ---- actions ----

    pub fn set_utc_offset(&mut self, seconds: i32) {
        self.utc_offset = seconds;
        self.act(|core| {
            core.set_utc_offset(seconds);
            Ok(())
        });
    }

    /// `mailbox_json` is a sidebar item's `mailbox`. Anything unreadable shows the inbox.
    pub fn select_mailbox(&mut self, mailbox_json: &str) {
        self.mailbox = serde_json::from_str(mailbox_json).unwrap_or(Mailbox::Inbox);
        self.bump();
    }

    /// "threads" or "people"; anything else means threads.
    pub fn set_list_mode(&mut self, mode: &str) {
        self.mode = if mode == "people" {
            "people"
        } else {
            "threads"
        }
        .into();
        self.bump();
    }

    pub fn open_conversation(&mut self, id: &str) {
        self.act(|core| core.mark_read(id));
    }

    pub fn toggle_pin(&mut self, id: &str) {
        self.act(|core| core.toggle_pin(id));
    }

    pub fn mark_done(&mut self, id: &str) {
        self.act(|core| core.mark_done(id));
    }

    pub fn undo_done(&mut self, id: &str) {
        self.act(|core| core.undo_done(id));
    }

    pub fn mark_unread(&mut self, id: &str) {
        self.act(|core| core.mark_unread(id));
    }

    /// Queue a reply. Returns the pending send as JSON, or empty on failure.
    pub fn send_reply(&mut self, id: &str, text: &str) -> String {
        let mut out = String::new();
        self.act(|core| {
            out = json(&core.send_reply(id, text)?);
            Ok(())
        });
        out
    }

    /// Cancel a send. Returns its text to put back in the reply box, or
    /// empty if it already went out.
    pub fn undo_send(&mut self, outbox_id: i64) -> String {
        let mut out = String::new();
        self.act(|core| {
            out = core.undo_send(outbox_id)?.unwrap_or_default();
            Ok(())
        });
        out
    }

    /// Bring in synced mail and send what is due. Call it about once a
    /// second. Returns true if anything on screen changed.
    pub fn tick(&mut self) -> bool {
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        match session.tick() {
            Ok(false) => false,
            Ok(true) => {
                self.bump();
                true
            }
            Err(e) => {
                self.error = e.to_string();
                self.bump();
                true
            }
        }
    }

    /// `provider` is "google", "microsoft" or "icloud" (which uses the email and password).
    pub fn start_sign_in(&mut self, provider: &str, email: &str, password: &str) {
        let request = match provider {
            "google" => NewAccount::Google,
            "microsoft" => NewAccount::Microsoft,
            _ => NewAccount::ICloud {
                email: email.to_string(),
                password: password.to_string(),
            },
        };
        if let Some(session) = self.session.as_mut() {
            session.sign_in(request);
        }
        self.bump();
    }

    pub fn dismiss_sign_in(&mut self) {
        if let Some(session) = self.session.as_mut() {
            session.dismiss_sign_in();
        }
        self.bump();
    }

    pub fn remove_account(&mut self, id: &str) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.error = session
            .remove_account(id)
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        self.mailbox = Mailbox::Inbox;
        self.bump();
    }

    pub fn sync_now(&self) {
        if let Some(session) = self.session.as_ref() {
            session.sync_now();
        }
    }

    /// Switch to the offline demo.
    pub fn use_demo(&mut self) {
        let opened = Session::demo(self.utc_offset);
        self.switch_session(opened);
    }

    /// Open the real accounts: at launch, leaving the demo, or retrying after an error.
    pub fn open_accounts(&mut self) {
        // Close the current session first: it may hold the database open.
        self.session = None;
        let opened = (self.open_real)(self.utc_offset);
        self.switch_session(opened);
    }

    /// Delete the mail stored on this computer and open an empty mailbox.
    pub fn start_fresh(&mut self) {
        self.session = None;
        let cleared = match &self.data_dir {
            Some(dir) => Session::start_fresh(dir),
            None => Err(no_home()),
        };
        match cleared {
            Ok(()) => self.open_accounts(),
            Err(e) => self.switch_session(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests;
