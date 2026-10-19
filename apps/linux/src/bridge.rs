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

        /// Called once a second by QML: sends whatever is due.
        #[qinvokable]
        fn tick(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "lastError"]
        fn last_error(&self) -> QString;
    }
}

use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use lp_app::Core;
use lp_app::view::Mailbox;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn json<T: serde::Serialize>(value: &T) -> QString {
    QString::from(serde_json::to_string(value).unwrap_or_else(|_| "null".into()).as_str())
}

pub struct AppControllerRust {
    core: Option<Core>,
    error: String,
    revision: i32,
    mode: QString,
    mailbox: QString,
    demo: bool,
}

impl Default for AppControllerRust {
    fn default() -> Self {
        // v0.1 starts in demo mode until account sign-in lands.
        let (core, error) = match lp_app::demo::demo_core(Box::new(now), 0) {
            Ok(core) => (Some(core), String::new()),
            Err(e) => (None, e.to_string()),
        };
        Self {
            core,
            error,
            revision: 0,
            mode: QString::from("threads"),
            mailbox: QString::from(r#"{"kind":"inbox"}"#),
            demo: true,
        }
    }
}

impl AppControllerRust {
    fn current_mailbox(&self) -> Mailbox {
        serde_json::from_str(&self.mailbox.to_string()).unwrap_or(Mailbox::Inbox)
    }
}

impl qobject::AppController {
    fn bump(mut self: Pin<&mut Self>) {
        let next = self.revision().wrapping_add(1);
        self.as_mut().set_revision(next);
    }

    /// Run an action on the core, record any error, and refresh the views.
    fn act(mut self: Pin<&mut Self>, f: impl FnOnce(&mut Core) -> lp_app::Result<()>) {
        let result = match self.as_mut().rust_mut().core.as_mut() {
            Some(core) => f(core),
            None => return,
        };
        self.as_mut().rust_mut().error = result.err().map(|e| e.to_string()).unwrap_or_default();
        self.bump();
    }

    fn set_utc_offset(self: Pin<&mut Self>, seconds: i32) {
        self.act(|core| {
            core.set_utc_offset(seconds);
            Ok(())
        });
    }

    fn sidebar_json(&self) -> QString {
        match &self.rust().core {
            Some(core) => json(&core.sidebar()),
            None => QString::from("null"),
        }
    }

    fn list_json(&self) -> QString {
        let Some(core) = &self.rust().core else {
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
        let Some(core) = &self.rust().core else {
            return QString::from("null");
        };
        let conversation = match id.strip_prefix("people:") {
            Some(key) => core.person_conversation(key),
            None => core.conversation(&id),
        };
        json(&conversation)
    }

    fn message_html(&self, id: i64) -> QString {
        let html = self.rust().core.as_ref().and_then(|core| core.message_html(id));
        QString::from(html.unwrap_or_default().as_str())
    }

    fn pending_json(&self) -> QString {
        match &self.rust().core {
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
        let processed = match self.as_mut().rust_mut().core.as_mut() {
            Some(core) => core.process_outbox(),
            None => return,
        };
        match processed {
            Ok(0) => {}
            Ok(_) => self.bump(),
            Err(e) => {
                self.as_mut().rust_mut().error = e.to_string();
                self.bump();
            }
        }
    }

    fn last_error(&self) -> QString {
        QString::from(self.rust().error.as_str())
    }
}
