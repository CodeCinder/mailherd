//! One account's background thread.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use mh_app::sync::{MailProvider, ProviderError, SyncEvent, SyncStatus};
use mh_app::{JobResult, OutboxJob, Work};
use mh_model::{Account, ProviderKind};

use crate::{Command, Connector, Timing};

type Provider = Option<Box<dyn MailProvider>>;

pub(crate) struct Worker {
    pub account: Account,
    pub connector: Arc<dyn Connector>,
    pub timing: Timing,
    pub cursor: Option<String>,
    pub events: Sender<SyncEvent>,
    pub results: Sender<JobResult>,
    pub commands: Receiver<Command>,
}

/// The provider, connecting first if needed.
fn connected<'a>(
    connector: &dyn Connector,
    account: &Account,
    provider: &'a mut Provider,
) -> Result<&'a mut Box<dyn MailProvider>, ProviderError> {
    if provider.is_none() {
        *provider = Some(connector.connect(account)?);
    }
    provider
        .as_mut()
        .ok_or_else(|| ProviderError::Failed("no connection".into()))
}

impl Worker {
    pub fn run(mut self) {
        let mut provider: Provider = None;
        let mut failures: u32 = 0;
        let mut renewed = false;
        loop {
            let wait = match self.sync_pass(&mut provider) {
                Ok(cursor) => {
                    failures = 0;
                    renewed = false;
                    self.send(SyncEvent::Cursor {
                        account_id: self.account.id.clone(),
                        cursor: cursor.clone(),
                    });
                    self.cursor = Some(cursor);
                    Some(self.poll_interval())
                }
                // Access tokens can be rejected before they expire; get a
                // fresh one and try once more before bothering the user.
                Err(ProviderError::NeedsSignIn) if !renewed => {
                    renewed = true;
                    provider = None;
                    self.connector.renew(&self.account);
                    Some(Duration::ZERO)
                }
                Err(ProviderError::NeedsSignIn) => {
                    provider = None;
                    self.status(SyncStatus::NeedsSignIn);
                    // Nothing to do until the user signs in again.
                    None
                }
                Err(ProviderError::Retry(message) | ProviderError::Failed(message)) => {
                    failures = failures.saturating_add(1);
                    provider = None;
                    self.status(SyncStatus::Error { message });
                    Some(self.backoff(failures))
                }
            };
            if !self.wait(wait, &mut provider) {
                return;
            }
        }
    }

    fn sync_pass(&mut self, provider: &mut Provider) -> Result<String, ProviderError> {
        let events = self.events.clone();
        let p = connected(&*self.connector, &self.account, provider)?;
        p.sync(self.cursor.as_deref(), &mut |e| {
            let _ = events.send(e);
        })
    }

    /// Wait until the next sync is due, running outbox jobs as they arrive.
    /// `None` waits for a command. Returns false when the worker should stop.
    fn wait(&mut self, wait: Option<Duration>, provider: &mut Provider) -> bool {
        let mut deadline = wait.map(|w| Instant::now() + w);
        loop {
            let command = match deadline {
                Some(d) => match self
                    .commands
                    .recv_timeout(d.saturating_duration_since(Instant::now()))
                {
                    Ok(c) => c,
                    Err(RecvTimeoutError::Timeout) => return true,
                    Err(RecvTimeoutError::Disconnected) => return false,
                },
                None => match self.commands.recv() {
                    Ok(c) => c,
                    Err(_) => return false,
                },
            };
            match command {
                Command::Job(job) => {
                    if self.run_job(job, provider) {
                        let soon = Instant::now() + self.timing.after_job;
                        deadline = Some(deadline.map_or(soon, |d| d.min(soon)));
                    }
                }
                Command::SyncNow => return true,
                Command::Stop => return false,
            }
        }
    }

    fn run_job(&mut self, job: OutboxJob, provider: &mut Provider) -> bool {
        let result =
            connected(&*self.connector, &self.account, provider).and_then(|p| match &job.work {
                Work::Send { raw } => p.send(raw),
                Work::Apply(action) => p.apply(action),
            });
        if result.is_err() {
            // Reconnect next time, in case the connection itself went bad.
            *provider = None;
        }
        let ok = result.is_ok();
        let _ = self.results.send(JobResult {
            id: job.id,
            result: result.map_err(|e| e.to_string()),
        });
        ok
    }

    fn poll_interval(&self) -> Duration {
        match self.account.kind {
            ProviderKind::ICloud => self.timing.poll_imap,
            ProviderKind::Gmail | ProviderKind::Microsoft365 => self.timing.poll,
        }
    }

    fn backoff(&self, failures: u32) -> Duration {
        let factor = 1u32 << failures.saturating_sub(1).min(10);
        self.timing
            .retry
            .saturating_mul(factor)
            .min(self.timing.retry_max)
    }

    fn status(&self, status: SyncStatus) {
        self.send(SyncEvent::Status {
            account_id: self.account.id.clone(),
            status,
        });
    }

    fn send(&self, event: SyncEvent) {
        let _ = self.events.send(event);
    }
}
