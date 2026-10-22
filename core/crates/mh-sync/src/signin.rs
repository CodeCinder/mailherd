//! Adding an account: browser sign-in for Google and Microsoft, an
//! app-specific password for iCloud. The slow part runs on its own thread.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use mh_app::sync::ProviderError;
use mh_auth::http::Http;
use mh_auth::{AuthError, Credential, SignIn, Vault};
use mh_model::{Account, ProviderKind, normalize_email};
use serde::Serialize;

use crate::Connector;

/// The account the user asked to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewAccount {
    Google,
    Microsoft,
    ICloud { email: String, password: String },
}

/// Progress of adding an account, shown on the sign-in sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SignInState {
    /// Finish signing in on this page in the browser.
    Browser {
        url: String,
    },
    /// Checking the details with the provider.
    Checking,
    Done {
        email: String,
    },
    Failed {
        message: String,
    },
}

fn provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Gmail => "Google",
        ProviderKind::Microsoft365 => "Microsoft",
        ProviderKind::ICloud => "iCloud",
    }
}

fn color(kind: ProviderKind) -> u32 {
    match kind {
        ProviderKind::ICloud => 0x34aadc,
        ProviderKind::Gmail => 0xea4335,
        ProviderKind::Microsoft365 => 0x0078d4,
    }
}

/// The account MailHerd already has for this mailbox, or a new one.
fn account_for(kind: ProviderKind, email: &str, existing: &[Account]) -> (Account, bool) {
    let wanted = normalize_email(email);
    if let Some(a) = existing
        .iter()
        .find(|a| a.kind == kind && normalize_email(&a.email) == wanted)
    {
        return (a.clone(), false);
    }
    let account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind,
        email: email.to_string(),
        display_name: "You".into(),
        color: color(kind),
    };
    (account, true)
}

fn auth_message(e: AuthError) -> String {
    match e {
        AuthError::Denied(_) => "Sign-in was cancelled.".into(),
        AuthError::TimedOut => "Sign-in took too long. Please try again.".into(),
        other => format!("Sign-in didn't work: {other}"),
    }
}

type Outcome = Result<(Account, bool), String>;

fn finish_browser(
    connector: &dyn Connector,
    kind: ProviderKind,
    sign_in: SignIn,
    timeout: Duration,
    existing: &[Account],
) -> Outcome {
    let tokens = sign_in
        .finish(timeout, &Http::new())
        .map_err(auth_message)?;
    let email = connector
        .whoami(kind, &tokens.access_token)
        .map_err(|e| format!("Signed in, but couldn't open the mailbox: {e}"))?;
    let (account, is_new) = account_for(kind, &email, existing);
    connector
        .vault()
        .save(&account.id, &Credential::OAuth { tokens })
        .map_err(|e| format!("Couldn't save the sign-in to the keychain: {e}"))?;
    Ok((account, is_new))
}

fn finish_icloud(
    connector: &dyn Connector,
    email: &str,
    password: &str,
    existing: &[Account],
) -> Outcome {
    if !email.contains('@') || password.is_empty() {
        return Err("Enter your iCloud email address and an app-specific password.".into());
    }
    connector
        .check_password(email, password)
        .map_err(|e| match e {
            ProviderError::NeedsSignIn => "iCloud didn't accept that email address and \
                 app-specific password. Check both and try again."
                .to_string(),
            ProviderError::Retry(m) => format!("Couldn't reach iCloud: {m}"),
            ProviderError::Failed(m) => m,
        })?;
    let (account, is_new) = account_for(ProviderKind::ICloud, email, existing);
    connector
        .vault()
        .save(
            &account.id,
            &Credential::Password {
                username: email.to_string(),
                password: password.to_string(),
            },
        )
        .map_err(|e| format!("Couldn't save the password to the keychain: {e}"))?;
    Ok((account, is_new))
}

struct Finished {
    generation: u64,
    outcome: Outcome,
}

/// The current sign-in, and results coming back from sign-in threads.
pub(crate) struct Tracker {
    generation: u64,
    state: Option<SignInState>,
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
}

impl Tracker {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            generation: 0,
            state: None,
            tx,
            rx,
        }
    }

    pub fn state(&self) -> Option<&SignInState> {
        self.state.as_ref()
    }

    pub fn dismiss(&mut self) {
        // Results from the abandoned attempt will no longer match.
        self.generation += 1;
        self.state = None;
    }

    pub fn start(
        &mut self,
        connector: Arc<dyn Connector>,
        request: NewAccount,
        existing: &[Account],
        timeout: Duration,
    ) -> &SignInState {
        self.generation += 1;
        let generation = self.generation;
        let tx = self.tx.clone();
        let existing = existing.to_vec();
        let state = match request {
            NewAccount::Google | NewAccount::Microsoft => {
                let kind = if request == NewAccount::Google {
                    ProviderKind::Gmail
                } else {
                    ProviderKind::Microsoft365
                };
                match connector.oauth_client(kind).map(|c| SignIn::start(&c)) {
                    None => SignInState::Failed {
                        message: format!(
                            "This build of MailHerd isn't set up to sign in to {} yet.",
                            provider_name(kind)
                        ),
                    },
                    Some(Err(e)) => SignInState::Failed {
                        message: auth_message(e),
                    },
                    Some(Ok(sign_in)) => {
                        let url = sign_in.url().to_string();
                        spawn(tx, generation, move || {
                            finish_browser(&*connector, kind, sign_in, timeout, &existing)
                        })
                        .unwrap_or(SignInState::Browser { url })
                    }
                }
            }
            NewAccount::ICloud { email, password } => spawn(tx, generation, move || {
                finish_icloud(&*connector, email.trim(), password.trim(), &existing)
            })
            .unwrap_or(SignInState::Checking),
        };
        self.state.insert(state)
    }

    /// Accounts whose sign-in finished since the last call, with whether each
    /// is new. Updates the state shown to the user.
    pub fn finished(&mut self, vault: &Vault) -> Vec<(Account, bool)> {
        let mut done = Vec::new();
        for Finished {
            generation,
            outcome,
        } in self.rx.try_iter()
        {
            if generation != self.generation {
                // The user closed the sheet before this attempt finished.
                if let Ok((account, true)) = outcome {
                    let _ = vault.forget(&account.id);
                }
                continue;
            }
            self.state = Some(match outcome {
                Ok((account, is_new)) => {
                    let email = account.email.clone();
                    done.push((account, is_new));
                    SignInState::Done { email }
                }
                Err(message) => SignInState::Failed { message },
            });
        }
        done
    }
}

/// Run `work` on its own thread. Returns a failure state if the thread
/// couldn't start, otherwise `None`.
fn spawn(
    tx: Sender<Finished>,
    generation: u64,
    work: impl FnOnce() -> Outcome + Send + 'static,
) -> Option<SignInState> {
    std::thread::Builder::new()
        .name("sign-in".into())
        .spawn(move || {
            let outcome = work();
            let _ = tx.send(Finished {
                generation,
                outcome,
            });
        })
        .err()
        .map(|e| SignInState::Failed {
            message: e.to_string(),
        })
}
