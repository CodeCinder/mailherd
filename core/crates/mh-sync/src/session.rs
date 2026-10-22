//! Starting MailHerd: the encrypted mail database, the core and the sync
//! engine together. Each UI creates one [`Session`] and calls [`Session::tick`]
//! on a timer.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mh_app::Core;
use mh_auth::{MemoryStore, Vault, VaultError};
use mh_store::{Store, StoreError};

use crate::{Connector, Engine, Live, NewAccount, SignInState, Timing};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("MailHerd couldn't use the system keychain: {0}")]
    Keychain(#[from] VaultError),
    #[error("MailHerd couldn't open its mail database: {0}")]
    Store(#[from] StoreError),
    #[error(transparent)]
    Core(#[from] mh_app::CoreError),
    #[error("MailHerd couldn't create its data folder: {0}")]
    Io(#[from] std::io::Error),
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Where MailHerd keeps its database on this platform.
pub fn default_data_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if cfg!(target_os = "macos") {
        return env("HOME").map(|h| h.join("Library/Application Support/MailHerd"));
    }
    if cfg!(windows) {
        return env("APPDATA").map(|a| a.join("MailHerd"));
    }
    env("XDG_DATA_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".local/share")))
        .map(|d| d.join("mailherd"))
}

pub struct Session {
    core: Core,
    /// `None` in the demo, which never goes online.
    engine: Option<Engine>,
    shown_sign_in: Option<SignInState>,
}

impl Session {
    /// The everyday app: mail kept in `data_dir`, encrypted with a key held
    /// in the system keychain, and every account syncing.
    pub fn open(data_dir: &Path, utc_offset: i32) -> Result<Self, SessionError> {
        let vault = Vault::keychain();
        let key = vault.database_key()?;
        std::fs::create_dir_all(data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(data_dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let store = Store::open(&data_dir.join("mailherd.db"), &key)?;
        Self::start(
            store,
            Arc::new(Live::new(vault)),
            Timing::default(),
            utc_offset,
        )
    }

    /// Real accounts, but nothing saved: mail stays in memory and sign-ins
    /// are forgotten on quit. For trying MailHerd out.
    pub fn ephemeral(utc_offset: i32) -> Result<Self, SessionError> {
        let vault = Vault::new(Arc::new(MemoryStore::default()));
        Self::start(
            Store::open_in_memory()?,
            Arc::new(Live::new(vault)),
            Timing::default(),
            utc_offset,
        )
    }

    /// Sample conversations, offline. Nothing is sent.
    pub fn demo(utc_offset: i32) -> Result<Self, SessionError> {
        Ok(Self {
            core: mh_app::demo::demo_core(Box::new(now), utc_offset)?,
            engine: None,
            shown_sign_in: None,
        })
    }

    /// Build a session on any store and connector, and start syncing the
    /// accounts already in the store from where they left off.
    pub fn start(
        store: Store,
        connector: Arc<dyn Connector>,
        timing: Timing,
        utc_offset: i32,
    ) -> Result<Self, SessionError> {
        let (mut engine, transport) = Engine::new(connector, timing);
        let core = Core::new(store, Box::new(transport), Box::new(now), utc_offset)?;
        for account in core.accounts() {
            let cursor = core.store().sync_state(&account.id)?;
            engine.start_account(account, cursor);
        }
        Ok(Self {
            core,
            engine: Some(engine),
            shown_sign_in: None,
        })
    }

    pub fn core(&self) -> &Core {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    pub fn is_demo(&self) -> bool {
        self.engine.is_none()
    }

    /// Bring in what the accounts have synced and send what is due. Call it
    /// about once a second. Returns true if anything on screen should change.
    pub fn tick(&mut self) -> mh_app::Result<bool> {
        let mut changed = false;
        if let Some(engine) = &mut self.engine {
            let events = engine.poll();
            if !events.is_empty() {
                changed |= self.core.apply_sync(events)?;
            }
            let state = engine.sign_in_state();
            if state != self.shown_sign_in.as_ref() {
                self.shown_sign_in = state.cloned();
                changed = true;
            }
        }
        changed |= self.core.process_outbox()? > 0;
        Ok(changed)
    }

    /// Start adding an account (or signing in again to one).
    pub fn sign_in(&mut self, request: NewAccount) -> Option<SignInState> {
        let engine = self.engine.as_mut()?;
        let state = engine.sign_in(request, self.core.accounts()).clone();
        self.shown_sign_in = Some(state.clone());
        Some(state)
    }

    pub fn sign_in_state(&self) -> Option<&SignInState> {
        self.engine.as_ref()?.sign_in_state()
    }

    pub fn dismiss_sign_in(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.dismiss_sign_in();
        }
        self.shown_sign_in = None;
    }

    /// Check every account for new mail now.
    pub fn sync_now(&self) {
        if let Some(engine) = &self.engine {
            engine.sync_now(None);
        }
    }

    /// Stop syncing an account and remove it, its saved sign-in and its mail
    /// from this device. Nothing changes on the server.
    pub fn remove_account(&mut self, account_id: &str) -> Result<(), SessionError> {
        if let Some(engine) = &mut self.engine {
            engine.remove_account(account_id)?;
        }
        self.core.remove_account(account_id)?;
        Ok(())
    }
}
