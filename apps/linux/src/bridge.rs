//! The bridge between the Rust core and QML.
//!
//! `AppController` is the only object QML talks to. View models cross as
//! JSON strings that QML turns into JavaScript objects; actions come back as
//! method calls. `revision` changes whenever the views should be re-read.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, revision)]
        #[qproperty(QString, mode)]
        #[qproperty(QString, mailbox)]
        #[qproperty(bool, demo)]
        type AppController = super::AppControllerRust;

        #[qinvokable]
        #[cxx_name = "setUtcOffset"]
        fn set_utc_offset(self: Pin<&mut Self>, seconds: i32);

        #[qinvokable]
        #[cxx_name = "sidebarJson"]
        fn sidebar_json(&self) -> QString;

        #[qinvokable]
        #[cxx_name = "listJson"]
        fn list_json(&self) -> QString;

        #[qinvokable]
        #[cxx_name = "conversationJson"]
        fn conversation_json(&self, id: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "messageHtml"]
        fn message_html(&self, id: i64) -> QString;

        #[qinvokable]
        #[cxx_name = "pendingJson"]
        fn pending_json(&self) -> QString;

        #[qinvokable]
        #[cxx_name = "selectMailbox"]
        fn select_mailbox(self: Pin<&mut Self>, mailbox_json: &QString);

        #[qinvokable]
        #[cxx_name = "setListMode"]
        fn set_list_mode(self: Pin<&mut Self>, mode: &QString);

        #[qinvokable]
        #[cxx_name = "openConversation"]
        fn open_conversation(self: Pin<&mut Self>, id: &QString);

        #[qinvokable]
        #[cxx_name = "togglePin"]
        fn toggle_pin(self: Pin<&mut Self>, id: &QString);

        #[qinvokable]
        #[cxx_name = "markDone"]
        fn mark_done(self: Pin<&mut Self>, id: &QString);

        #[qinvokable]
        #[cxx_name = "undoDone"]
        fn undo_done(self: Pin<&mut Self>, id: &QString);

        #[qinvokable]
        #[cxx_name = "markUnread"]
        fn mark_unread(self: Pin<&mut Self>, id: &QString);

        /// Returns the pending send as JSON, or an empty string on failure.
        #[qinvokable]
        #[cxx_name = "sendReply"]
        fn send_reply(self: Pin<&mut Self>, id: &QString, text: &QString) -> QString;

        /// Returns the text of the cancelled message, or an empty string if it already went out.
        #[qinvokable]
        #[cxx_name = "undoSend"]
        fn undo_send(self: Pin<&mut Self>, outbox_id: i32) -> QString;

        /// Called once a second by QML: brings in synced mail, sends whatever is due.
        #[qinvokable]
        fn tick(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "lastError"]
        fn last_error(&self) -> QString;

        /// Why MailHerd couldn't open its mail database, or an empty string.
        #[qinvokable]
        #[cxx_name = "startupError"]
        fn startup_error(&self) -> QString;

        /// True when the saved mail can't be unlocked and `startFresh` is the way out.
        #[qinvokable]
        #[cxx_name = "canStartFresh"]
        fn can_start_fresh(&self) -> bool;

        /// Delete the mail stored on this computer and open an empty mailbox.
        #[qinvokable]
        #[cxx_name = "startFresh"]
        fn start_fresh(self: Pin<&mut Self>);

        /// The account being added, as JSON, or "null".
        #[qinvokable]
        #[cxx_name = "signInJson"]
        fn sign_in_json(&self) -> QString;

        /// `provider` is "google", "microsoft" or "icloud" (which uses the email and password).
        #[qinvokable]
        #[cxx_name = "startSignIn"]
        fn start_sign_in(
            self: Pin<&mut Self>,
            provider: &QString,
            email: &QString,
            password: &QString,
        );

        #[qinvokable]
        #[cxx_name = "dismissSignIn"]
        fn dismiss_sign_in(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "removeAccount"]
        fn remove_account(self: Pin<&mut Self>, id: &QString);

        #[qinvokable]
        #[cxx_name = "syncNow"]
        fn sync_now(self: Pin<&mut Self>);

        /// Switch to the offline demo.
        #[qinvokable]
        #[cxx_name = "useDemo"]
        fn use_demo(self: Pin<&mut Self>);

        /// Leave the demo (or retry after a startup error) and open the real accounts.
        #[qinvokable]
        #[cxx_name = "openAccounts"]
        fn open_accounts(self: Pin<&mut Self>);
    }
}

use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use mh_app::Core;
use mh_app::view::Mailbox;
use mh_sync::{NewAccount, Session, SessionError};

const INBOX: &str = r#"{"kind":"inbox"}"#;

fn json<T: serde::Serialize>(value: &T) -> QString {
    QString::from(serde_json::to_string(value).unwrap_or_else(|_| "null".into()).as_str())
}

fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

/// The real accounts. `MAILHERD_EPHEMERAL=1` keeps everything in memory
/// instead, for trying sign-in without saving anything.
fn open_real(utc_offset: i32) -> Result<Session, SessionError> {
    if flag("MAILHERD_EPHEMERAL") {
        return Session::ephemeral(utc_offset);
    }
    let dir = mh_sync::default_data_dir()
        .ok_or_else(|| std::io::Error::other("there is no home folder"))?;
    Session::open(&dir, utc_offset)
}

pub struct AppControllerRust {
    session: Option<Session>,
    error: String,
    startup_error: String,
    can_start_fresh: bool,
    utc_offset: i32,
    revision: i32,
    mode: QString,
    mailbox: QString,
    demo: bool,
}

impl Default for AppControllerRust {
    fn default() -> Self {
        let mut this = Self {
            session: None,
            error: String::new(),
            startup_error: String::new(),
            can_start_fresh: false,
            utc_offset: 0,
            revision: 0,
            mode: QString::from("threads"),
            mailbox: QString::from(INBOX),
            demo: false,
        };
        // MAILHERD_DEMO=1 opens straight into the demo (used for screenshots).
        let opened = if flag("MAILHERD_DEMO") {
            Session::demo(0)
        } else {
            open_real(0)
        };
        this.demo = this.replace_session(opened);
        this
    }
}

impl AppControllerRust {
    fn current_mailbox(&self) -> Mailbox {
        serde_json::from_str(&self.mailbox.to_string()).unwrap_or(Mailbox::Inbox)
    }

    fn core(&self) -> Option<&Core> {
        self.session.as_ref().map(Session::core)
    }

    /// Swap in a new session. Returns whether it is the demo.
    fn replace_session(&mut self, opened: Result<Session, SessionError>) -> bool {
        // Stop the old session's sync before the new one opens the database.
        self.session = None;
        match opened {
            Ok(mut session) => {
                session.core_mut().set_utc_offset(self.utc_offset);
                self.startup_error.clear();
                self.can_start_fresh = false;
                let demo = session.is_demo();
                self.session = Some(session);
                demo
            }
            Err(e) => {
                self.startup_error = e.to_string();
                self.can_start_fresh = e.can_start_fresh();
                false
            }
        }
    }
}

impl qobject::AppController {
    fn bump(mut self: Pin<&mut Self>) {
        let next = self.revision().wrapping_add(1);
        self.as_mut().set_revision(next);
    }

    /// Run an action on the core, record any error, and refresh the views.
    fn act(mut self: Pin<&mut Self>, f: impl FnOnce(&mut Core) -> mh_app::Result<()>) {
        let result = match self.as_mut().rust_mut().session.as_mut() {
            Some(session) => f(session.core_mut()),
            None => return,
        };
        self.as_mut().rust_mut().error = result.err().map(|e| e.to_string()).unwrap_or_default();
        self.bump();
    }

    fn set_utc_offset(mut self: Pin<&mut Self>, seconds: i32) {
        self.as_mut().rust_mut().utc_offset = seconds;
        self.act(|core| {
            core.set_utc_offset(seconds);
            Ok(())
        });
    }

    fn sidebar_json(&self) -> QString {
        match self.rust().core() {
            Some(core) => json(&core.sidebar()),
            None => QString::from("null"),
        }
    }

    fn list_json(&self) -> QString {
        let Some(core) = self.rust().core() else {
            return QString::from("[]");
        };
        let mailbox = self.rust().current_mailbox();
        if self.rust().mode.to_string() == "people" {
            json(&core.people_list(&mailbox))
        } else {
            json(&core.thread_list(&mailbox))
        }
    }

    fn conversation_json(&self, id: &QString) -> QString {
        let id = id.to_string();
        let Some(core) = self.rust().core() else {
            return QString::from("null");
        };
        let conversation = match id.strip_prefix("people:") {
            Some(key) => core.person_conversation(key),
            None => core.conversation(&id),
        };
        json(&conversation)
    }

    fn message_html(&self, id: i64) -> QString {
        let html = self.rust().core().and_then(|core| core.message_html(id));
        QString::from(html.unwrap_or_default().as_str())
    }

    fn pending_json(&self) -> QString {
        match self.rust().core() {
            Some(core) => json(&core.pending_sends()),
            None => QString::from("[]"),
        }
    }

    fn select_mailbox(mut self: Pin<&mut Self>, mailbox_json: &QString) {
        self.as_mut().set_mailbox(mailbox_json.clone());
        self.bump();
    }

    fn set_list_mode(mut self: Pin<&mut Self>, mode: &QString) {
        self.as_mut().set_mode(mode.clone());
        self.bump();
    }

    fn open_conversation(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.act(|core| core.mark_read(&id));
    }

    fn toggle_pin(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.act(|core| core.toggle_pin(&id));
    }

    fn mark_done(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.act(|core| core.mark_done(&id));
    }

    fn undo_done(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.act(|core| core.undo_done(&id));
    }

    fn mark_unread(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.act(|core| core.mark_unread(&id));
    }

    fn send_reply(mut self: Pin<&mut Self>, id: &QString, text: &QString) -> QString {
        let (id, text) = (id.to_string(), text.to_string());
        let mut out = QString::from("");
        self.as_mut().act(|core| {
            let pending = core.send_reply(&id, &text)?;
            out = json(&pending);
            Ok(())
        });
        out
    }

    fn undo_send(mut self: Pin<&mut Self>, outbox_id: i32) -> QString {
        let mut out = QString::from("");
        self.as_mut().act(|core| {
            if let Some(text) = core.undo_send(i64::from(outbox_id))? {
                out = QString::from(text.as_str());
            }
            Ok(())
        });
        out
    }

    fn tick(mut self: Pin<&mut Self>) {
        let changed = match self.as_mut().rust_mut().session.as_mut() {
            Some(session) => session.tick(),
            None => return,
        };
        match changed {
            Ok(false) => {}
            Ok(true) => self.bump(),
            Err(e) => {
                self.as_mut().rust_mut().error = e.to_string();
                self.bump();
            }
        }
    }

    fn last_error(&self) -> QString {
        QString::from(self.rust().error.as_str())
    }

    fn startup_error(&self) -> QString {
        QString::from(self.rust().startup_error.as_str())
    }

    fn can_start_fresh(&self) -> bool {
        self.rust().can_start_fresh
    }

    fn start_fresh(self: Pin<&mut Self>) {
        let offset = self.rust().utc_offset;
        let opened = match mh_sync::default_data_dir() {
            Some(dir) => Session::start_fresh(&dir).and_then(|()| Session::open(&dir, offset)),
            None => Err(std::io::Error::other("there is no home folder").into()),
        };
        self.switch_session(opened);
    }

    fn sign_in_json(&self) -> QString {
        match self.rust().session.as_ref().and_then(Session::sign_in_state) {
            Some(state) => json(state),
            None => QString::from("null"),
        }
    }

    fn start_sign_in(
        mut self: Pin<&mut Self>,
        provider: &QString,
        email: &QString,
        password: &QString,
    ) {
        let request = match provider.to_string().as_str() {
            "google" => NewAccount::Google,
            "microsoft" => NewAccount::Microsoft,
            _ => NewAccount::ICloud {
                email: email.to_string(),
                password: password.to_string(),
            },
        };
        if let Some(session) = self.as_mut().rust_mut().session.as_mut() {
            session.sign_in(request);
        }
        self.bump();
    }

    fn dismiss_sign_in(mut self: Pin<&mut Self>) {
        if let Some(session) = self.as_mut().rust_mut().session.as_mut() {
            session.dismiss_sign_in();
        }
        self.bump();
    }

    fn remove_account(mut self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        let result = match self.as_mut().rust_mut().session.as_mut() {
            Some(session) => session.remove_account(&id),
            None => return,
        };
        self.as_mut().rust_mut().error = result.err().map(|e| e.to_string()).unwrap_or_default();
        self.as_mut().set_mailbox(QString::from(INBOX));
        self.bump();
    }

    fn sync_now(self: Pin<&mut Self>) {
        if let Some(session) = self.rust().session.as_ref() {
            session.sync_now();
        }
    }

    fn use_demo(self: Pin<&mut Self>) {
        let offset = self.rust().utc_offset;
        self.switch_session(Session::demo(offset));
    }

    fn open_accounts(self: Pin<&mut Self>) {
        let offset = self.rust().utc_offset;
        self.switch_session(open_real(offset));
    }

    fn switch_session(mut self: Pin<&mut Self>, opened: Result<Session, SessionError>) {
        let demo = self.as_mut().rust_mut().replace_session(opened);
        self.as_mut().set_demo(demo);
        self.as_mut().set_mailbox(QString::from(INBOX));
        self.bump();
    }
}
