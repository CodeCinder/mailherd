//! KDE Wallet through its own D-Bus interface, as KDE apps use it.
//!
//! KDE Wallet also offers the Secret Service interface that [`KeyringStore`]
//! uses, but on Kubuntu MailHerd's entries didn't survive a restart that way.
//! This interface is what KDE's own apps have used for years, and it can be
//! told to save to disk straight away (`sync`) instead of a few seconds later.
//!
//! [`KeyringStore`]: crate::KeyringStore

use std::sync::OnceLock;

use zbus::blocking::{Connection, Proxy};

use crate::secrets::{SecretError, SecretStore};

/// Entries go in this folder of the user's wallet, and requests name this app.
const FOLDER: &str = "MailHerd";
const APP_ID: &str = "MailHerd";

/// The daemon's D-Bus name and object path: Plasma 6, then Plasma 5.
const SERVICES: [(&str, &str); 2] = [
    ("org.kde.kwalletd6", "/modules/kwalletd6"),
    ("org.kde.kwalletd5", "/modules/kwalletd5"),
];

fn unavailable(e: impl std::fmt::Display) -> SecretError {
    SecretError::Unavailable(format!("KDE Wallet: {e}"))
}

pub struct KWalletStore {
    conn: Connection,
    /// Which of [`SERVICES`] answered, found on first use.
    service: OnceLock<(&'static str, &'static str)>,
}

impl KWalletStore {
    /// Connect to the session bus. Fails if there is none.
    pub fn new() -> Result<Self, SecretError> {
        Ok(Self {
            conn: Connection::session().map_err(unavailable)?,
            service: OnceLock::new(),
        })
    }

    /// True if KDE Wallet is installed and turned on.
    pub fn is_enabled(&self) -> bool {
        self.proxy()
            .and_then(|p| p.call::<_, _, bool>("isEnabled", &()).map_err(unavailable))
            .unwrap_or(false)
    }

    fn proxy(&self) -> Result<Proxy<'_>, SecretError> {
        if let Some((name, path)) = self.service.get() {
            return Proxy::new(&self.conn, *name, *path, "org.kde.KWallet").map_err(unavailable);
        }
        let mut last = None;
        for (name, path) in SERVICES {
            let proxy =
                Proxy::new(&self.conn, name, path, "org.kde.KWallet").map_err(unavailable)?;
            // Starts the daemon if it isn't running yet.
            match proxy.call::<_, _, bool>("isEnabled", &()) {
                Ok(_) => {
                    let _ = self.service.set((name, path));
                    return Ok(proxy);
                }
                Err(e) => last = Some(e),
            }
        }
        Err(unavailable(
            last.map_or("not installed".to_string(), |e| e.to_string()),
        ))
    }

    /// Open the user's wallet (KDE asks for its password if it is locked)
    /// and make sure MailHerd's folder exists. Returns the wallet handle.
    fn open(&self, proxy: &Proxy<'_>) -> Result<i32, SecretError> {
        let wallet: String = proxy.call("networkWallet", &()).map_err(unavailable)?;
        let handle: i32 = proxy
            .call("open", &(wallet.as_str(), 0i64, APP_ID))
            .map_err(unavailable)?;
        if handle < 0 {
            return Err(SecretError::Unavailable(
                "KDE Wallet stayed locked (the password prompt was closed)".into(),
            ));
        }
        let has: bool = proxy
            .call("hasFolder", &(handle, FOLDER, APP_ID))
            .map_err(unavailable)?;
        if !has {
            let made: bool = proxy
                .call("createFolder", &(handle, FOLDER, APP_ID))
                .map_err(unavailable)?;
            if !made {
                return Err(SecretError::Other(
                    "KDE Wallet refused to create MailHerd's folder".into(),
                ));
            }
        }
        Ok(handle)
    }

    /// Write the wallet to disk now, so nothing is lost if the computer
    /// restarts in the next few seconds.
    fn sync(&self, proxy: &Proxy<'_>, handle: i32) -> Result<(), SecretError> {
        proxy
            .call::<_, _, ()>("sync", &(handle, APP_ID))
            .map_err(unavailable)
    }
}

impl SecretStore for KWalletStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let proxy = self.proxy()?;
        let handle = self.open(&proxy)?;
        let has: bool = proxy
            .call("hasEntry", &(handle, FOLDER, key, APP_ID))
            .map_err(unavailable)?;
        if !has {
            return Ok(None);
        }
        let value: String = proxy
            .call("readPassword", &(handle, FOLDER, key, APP_ID))
            .map_err(unavailable)?;
        Ok(Some(value))
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let proxy = self.proxy()?;
        let handle = self.open(&proxy)?;
        let status: i32 = proxy
            .call("writePassword", &(handle, FOLDER, key, value, APP_ID))
            .map_err(unavailable)?;
        if status != 0 {
            return Err(SecretError::Other(format!(
                "KDE Wallet didn't save it (code {status})"
            )));
        }
        self.sync(&proxy, handle)
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let proxy = self.proxy()?;
        let handle = self.open(&proxy)?;
        let has: bool = proxy
            .call("hasEntry", &(handle, FOLDER, key, APP_ID))
            .map_err(unavailable)?;
        if has {
            let _: i32 = proxy
                .call("removeEntry", &(handle, FOLDER, key, APP_ID))
                .map_err(unavailable)?;
            self.sync(&proxy, handle)?;
        }
        Ok(())
    }
}
