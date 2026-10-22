//! Background sync for MailHerd.
//!
//! Each account gets a worker thread that owns its [`MailProvider`]: it syncs
//! on a timer, runs outbox jobs, and sends [`SyncEvent`]s back. The UI thread
//! owns the [`Engine`], drains those events with [`Engine::poll`] and hands
//! them to [`mh_app::Core::apply_sync`]. The core reaches the workers through
//! the [`SyncTransport`] it was built with.
//!
//! Adding an account also starts here: [`Engine::sign_in`] runs the browser
//! sign-in (Google, Microsoft) or the password check (iCloud) in the
//! background and reports progress as a [`SignInState`].

mod live;
mod signin;
mod worker;

pub use live::Live;
pub use signin::{NewAccount, SignInState};

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mh_app::sync::{MailProvider, ProviderError, SyncEvent, SyncStatus};
use mh_app::{JobResult, OutboxJob, Transport};
use mh_auth::{OAuthClient, Vault, VaultError};
use mh_model::{Account, AccountId, ProviderKind};

/// Everything the engine needs from the outside world. [`Live`] talks to the
/// real services; tests use fakes.
pub trait Connector: Send + Sync + 'static {
    /// A provider for a signed-in account, using its saved credential.
    fn connect(&self, account: &Account) -> Result<Box<dyn MailProvider>, ProviderError>;
    /// The address of the mailbox a fresh access token opens.
    fn whoami(&self, kind: ProviderKind, access_token: &str) -> Result<String, ProviderError>;
    /// Check an iCloud address and app-specific password.
    fn check_password(&self, email: &str, password: &str) -> Result<(), ProviderError>;
    /// The app registration for Google or Microsoft sign-in, if this build has one.
    fn oauth_client(&self, kind: ProviderKind) -> Option<OAuthClient>;
    fn vault(&self) -> &Vault;
    /// The provider rejected the saved access token: make the next request
    /// fetch a fresh one before giving up and asking the user to sign in.
    fn renew(&self, _account: &Account) {}
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// How often Gmail and Microsoft 365 are checked for changes.
    pub poll: Duration,
    /// How often iCloud is checked (each IMAP check logs in again).
    pub poll_imap: Duration,
    /// First wait after an error; doubles with each failure in a row.
    pub retry: Duration,
    pub retry_max: Duration,
    /// Pause after an outbox job before syncing, so its effect shows up.
    pub after_job: Duration,
    /// How long a browser sign-in waits for the user.
    pub sign_in_timeout: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(60),
            poll_imap: Duration::from_secs(120),
            retry: Duration::from_secs(30),
            retry_max: Duration::from_secs(600),
            after_job: Duration::from_secs(2),
            sign_in_timeout: Duration::from_secs(300),
        }
    }
}

enum Command {
    Job(OutboxJob),
    SyncNow,
    Stop,
}

type Workers = Arc<Mutex<HashMap<AccountId, Sender<Command>>>>;

fn lock(workers: &Workers) -> MutexGuard<'_, HashMap<AccountId, Sender<Command>>> {
    // Nothing can leave the map half-written, so a poisoned lock is still usable.
    workers.lock().unwrap_or_else(|e| e.into_inner())
}

/// The core's way to reach the account workers.
pub struct SyncTransport {
    workers: Workers,
    results_tx: Sender<JobResult>,
    results: Receiver<JobResult>,
}

impl Transport for SyncTransport {
    fn submit(&mut self, job: OutboxJob) {
        let id = job.id;
        let worker = lock(&self.workers).get(&job.account.id).cloned();
        let delivered = worker.is_some_and(|w| w.send(Command::Job(job)).is_ok());
        if !delivered {
            let _ = self.results_tx.send(JobResult {
                id,
                result: Err("the account isn't connected".into()),
            });
        }
    }

    fn poll(&mut self) -> Vec<JobResult> {
        self.results.try_iter().collect()
    }
}

pub struct Engine {
    connector: Arc<dyn Connector>,
    timing: Timing,
    workers: Workers,
    events_tx: Sender<SyncEvent>,
    events: Receiver<SyncEvent>,
    results_tx: Sender<JobResult>,
    sign_in: signin::Tracker,
}

impl Engine {
    /// The engine, and the transport to build the [`mh_app::Core`] with.
    pub fn new(connector: Arc<dyn Connector>, timing: Timing) -> (Self, SyncTransport) {
        let workers = Workers::default();
        let (events_tx, events) = mpsc::channel();
        let (results_tx, results) = mpsc::channel();
        let transport = SyncTransport {
            workers: Arc::clone(&workers),
            results_tx: results_tx.clone(),
            results,
        };
        let engine = Self {
            connector,
            timing,
            workers,
            events_tx,
            events,
            results_tx,
            sign_in: signin::Tracker::new(),
        };
        (engine, transport)
    }

    /// Start syncing an account, carrying on from `cursor` (the sync state
    /// saved in the store; `None` for a first sync).
    pub fn start_account(&mut self, account: &Account, cursor: Option<String>) {
        self.stop_account(&account.id);
        let (tx, commands) = mpsc::channel();
        let worker = worker::Worker {
            account: account.clone(),
            connector: Arc::clone(&self.connector),
            timing: self.timing,
            cursor,
            events: self.events_tx.clone(),
            results: self.results_tx.clone(),
            commands,
        };
        let spawned = std::thread::Builder::new()
            .name(format!("sync {}", account.email))
            .spawn(move || worker.run());
        match spawned {
            Ok(_) => {
                lock(&self.workers).insert(account.id.clone(), tx);
            }
            Err(e) => {
                let _ = self.events_tx.send(SyncEvent::Status {
                    account_id: account.id.clone(),
                    status: SyncStatus::Error {
                        message: e.to_string(),
                    },
                });
            }
        }
    }

    pub fn stop_account(&mut self, account_id: &str) {
        if let Some(worker) = lock(&self.workers).remove(account_id) {
            let _ = worker.send(Command::Stop);
        }
    }

    /// Stop syncing an account and delete its saved sign-in. The caller
    /// removes its mail with [`mh_app::Core::remove_account`].
    pub fn remove_account(&mut self, account_id: &str) -> Result<(), VaultError> {
        self.stop_account(account_id);
        self.connector.vault().forget(account_id)
    }

    /// Check one account (or all of them) for changes now.
    pub fn sync_now(&self, account_id: Option<&str>) {
        for (id, worker) in lock(&self.workers).iter() {
            if account_id.is_none_or(|a| a == id) {
                let _ = worker.send(Command::SyncNow);
            }
        }
    }

    pub fn is_running(&self, account_id: &str) -> bool {
        lock(&self.workers).contains_key(account_id)
    }

    /// Everything the workers and sign-ins have reported since the last call,
    /// ready for [`mh_app::Core::apply_sync`]. The UI calls this on a timer.
    pub fn poll(&mut self) -> Vec<SyncEvent> {
        let mut events = Vec::new();
        for (account, is_new) in self.sign_in.finished(self.connector.vault()) {
            if is_new || !self.is_running(&account.id) {
                events.push(SyncEvent::AccountAdded {
                    account: account.clone(),
                });
                self.start_account(&account, None);
            } else {
                // Signed in again: the worker reconnects with the new sign-in.
                self.sync_now(Some(&account.id));
            }
        }
        events.extend(self.events.try_iter());
        events
    }

    /// Start adding an account. `existing` lets signing in again to an
    /// account MailHerd already has reuse it instead of adding a copy.
    pub fn sign_in(&mut self, request: NewAccount, existing: &[Account]) -> &SignInState {
        self.sign_in.start(
            Arc::clone(&self.connector),
            request,
            existing,
            self.timing.sign_in_timeout,
        )
    }

    /// The sign-in in progress or just finished, for the UI to show.
    pub fn sign_in_state(&self) -> Option<&SignInState> {
        self.sign_in.state()
    }

    /// Close the sign-in sheet. A browser sign-in still open is abandoned.
    pub fn dismiss_sign_in(&mut self) {
        self.sign_in.dismiss();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        for (_, worker) in lock(&self.workers).drain() {
            let _ = worker.send(Command::Stop);
        }
    }
}

#[cfg(test)]
mod tests;
