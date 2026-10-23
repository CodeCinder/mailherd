//! The bridge between the Rust core and QML.
//!
//! `AppController` is the only object QML talks to. It wraps the core's
//! [`mh_ui::Controller`], which the Apple apps share: view models cross as
//! JSON strings that QML turns into JavaScript objects, and actions come back
//! as method calls. `revision` changes whenever the views should be re-read.

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

        /// No accounts yet, or MailHerd couldn't start.
        #[qinvokable]
        #[cxx_name = "showWelcome"]
        fn show_welcome(&self) -> bool;

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
use mh_ui::Controller;

fn q(s: &str) -> QString {
    QString::from(s)
}

pub struct AppControllerRust {
    controller: Controller,
    revision: i32,
    mode: QString,
    mailbox: QString,
    demo: bool,
}

impl Default for AppControllerRust {
    fn default() -> Self {
        let controller = Controller::launch(0);
        Self {
            revision: 0,
            mode: q(controller.mode()),
            mailbox: q(&controller.mailbox_json()),
            demo: controller.is_demo(),
            controller,
        }
    }
}

impl qobject::AppController {
    /// Run `f` on the controller, then tell QML what changed.
    fn with<R>(mut self: Pin<&mut Self>, f: impl FnOnce(&mut Controller) -> R) -> R {
        let before = self.rust().controller.revision();
        let out = f(&mut self.as_mut().rust_mut().controller);
        let c = &self.rust().controller;
        if c.revision() != before {
            let (mode, mailbox, demo) = (q(c.mode()), q(&c.mailbox_json()), c.is_demo());
            // QML only needs to see that it changed.
            let revision = (c.revision() & 0x7fff_ffff) as i32;
            self.as_mut().set_mode(mode);
            self.as_mut().set_mailbox(mailbox);
            self.as_mut().set_demo(demo);
            self.as_mut().set_revision(revision);
        }
        out
    }

    fn set_utc_offset(self: Pin<&mut Self>, seconds: i32) {
        self.with(|c| c.set_utc_offset(seconds));
    }

    fn sidebar_json(&self) -> QString {
        q(&self.rust().controller.sidebar_json())
    }

    fn list_json(&self) -> QString {
        q(&self.rust().controller.list_json())
    }

    fn conversation_json(&self, id: &QString) -> QString {
        q(&self.rust().controller.conversation_json(&id.to_string()))
    }

    fn message_html(&self, id: i64) -> QString {
        q(&self.rust().controller.message_html(id))
    }

    fn pending_json(&self) -> QString {
        q(&self.rust().controller.pending_json())
    }

    fn select_mailbox(self: Pin<&mut Self>, mailbox_json: &QString) {
        let mailbox = mailbox_json.to_string();
        self.with(|c| c.select_mailbox(&mailbox));
    }

    fn set_list_mode(self: Pin<&mut Self>, mode: &QString) {
        let mode = mode.to_string();
        self.with(|c| c.set_list_mode(&mode));
    }

    fn open_conversation(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.with(|c| c.open_conversation(&id));
    }

    fn toggle_pin(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.with(|c| c.toggle_pin(&id));
    }

    fn mark_done(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.with(|c| c.mark_done(&id));
    }

    fn undo_done(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.with(|c| c.undo_done(&id));
    }

    fn mark_unread(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.with(|c| c.mark_unread(&id));
    }

    fn send_reply(self: Pin<&mut Self>, id: &QString, text: &QString) -> QString {
        let (id, text) = (id.to_string(), text.to_string());
        q(&self.with(|c| c.send_reply(&id, &text)))
    }

    fn undo_send(self: Pin<&mut Self>, outbox_id: i32) -> QString {
        q(&self.with(|c| c.undo_send(i64::from(outbox_id))))
    }

    fn tick(self: Pin<&mut Self>) {
        self.with(Controller::tick);
    }

    fn last_error(&self) -> QString {
        q(self.rust().controller.last_error())
    }

    fn show_welcome(&self) -> bool {
        self.rust().controller.show_welcome()
    }

    fn startup_error(&self) -> QString {
        q(self.rust().controller.startup_error())
    }

    fn can_start_fresh(&self) -> bool {
        self.rust().controller.can_start_fresh()
    }

    fn start_fresh(self: Pin<&mut Self>) {
        self.with(Controller::start_fresh);
    }

    fn sign_in_json(&self) -> QString {
        q(&self.rust().controller.sign_in_json())
    }

    fn start_sign_in(
        self: Pin<&mut Self>,
        provider: &QString,
        email: &QString,
        password: &QString,
    ) {
        let (provider, email, password) =
            (provider.to_string(), email.to_string(), password.to_string());
        self.with(|c| c.start_sign_in(&provider, &email, &password));
    }

    fn dismiss_sign_in(self: Pin<&mut Self>) {
        self.with(Controller::dismiss_sign_in);
    }

    fn remove_account(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.with(|c| c.remove_account(&id));
    }

    fn sync_now(self: Pin<&mut Self>) {
        self.rust().controller.sync_now();
    }

    fn use_demo(self: Pin<&mut Self>) {
        self.with(Controller::use_demo);
    }

    fn open_accounts(self: Pin<&mut Self>) {
        self.with(Controller::open_accounts);
    }
}
