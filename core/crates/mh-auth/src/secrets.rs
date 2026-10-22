//! Credentials kept in the OS keychain (Secret Service on Linux: GNOME Keyring
//! or KWallet). Nothing secret is ever written to MailHerd's own files.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::http::Http;
use crate::oauth::{AuthError, OAuthClient, Tokens, refresh};

const SERVICE: &str = "MailHerd";
const DATABASE_KEY: &str = "database-key";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretError {
    #[error("the system keychain isn't available: {0}")]
    Unavailable(String),
    #[error("keychain error: {0}")]
    Other(String),
}

/// Where secrets live. The keychain in the app; memory in tests.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &str, value: &str) -> Result<(), SecretError>;
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}

pub struct KeyringStore;

fn keyring_error(e: keyring::Error) -> SecretError {
    match e {
        keyring::Error::NoDefaultStore
        | keyring::Error::NoStorageAccess(_)
        | keyring::Error::PlatformFailure(_) => SecretError::Unavailable(e.to_string()),
        other => SecretError::Other(other.to_string()),
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(keyring_error)?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keyring_error(e)),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        keyring::Entry::new(SERVICE, key)
            .and_then(|e| e.set_password(value))
            .map_err(keyring_error)
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(SERVICE, key).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_error(e)),
        }
    }
}

#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        Ok(self
            .values
            .lock()
            .map_err(|e| SecretError::Other(e.to_string()))?
            .get(key)
            .cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .map_err(|e| SecretError::Other(e.to_string()))?
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .map_err(|e| SecretError::Other(e.to_string()))?
            .remove(key);
        Ok(())
    }
}

/// How one account signs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Credential {
    OAuth {
        tokens: Tokens,
    },
    /// iCloud: the address and an app-specific password.
    Password {
        username: String,
        password: String,
    },
}

fn account_key(account_id: &str) -> String {
    format!("account:{account_id}")
}

/// Reads and writes account credentials and the database key.
#[derive(Clone)]
pub struct Vault {
    store: Arc<dyn SecretStore>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum VaultError {
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("no saved sign-in for this account")]
    Missing,
    #[error("saved sign-in is unreadable")]
    Corrupt,
    /// The keychain accepted a secret but gave back something else.
    #[error("the keychain didn't keep what MailHerd saved")]
    NotKept,
}

impl Vault {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store }
    }

    pub fn keychain() -> Self {
        Self::new(Arc::new(KeyringStore))
    }

    pub fn credential(&self, account_id: &str) -> Result<Credential, VaultError> {
        let raw = self
            .store
            .get(&account_key(account_id))?
            .ok_or(VaultError::Missing)?;
        serde_json::from_str(&raw).map_err(|_| VaultError::Corrupt)
    }

    pub fn save(&self, account_id: &str, credential: &Credential) -> Result<(), VaultError> {
        let raw = serde_json::to_string(credential).map_err(|_| VaultError::Corrupt)?;
        Ok(self.store.set(&account_key(account_id), &raw)?)
    }

    pub fn forget(&self, account_id: &str) -> Result<(), VaultError> {
        Ok(self.store.delete(&account_key(account_id))?)
    }

    /// A valid access token for an OAuth account, refreshing (and saving) it if needed.
    pub fn access_token(
        &self,
        account_id: &str,
        client: &OAuthClient,
        http: &Http,
        now: i64,
    ) -> Result<String, VaultError> {
        let Credential::OAuth { tokens } = self.credential(account_id)? else {
            return Err(VaultError::Missing);
        };
        if !tokens.expiring(now) {
            return Ok(tokens.access_token);
        }
        let refresh_token = tokens.refresh_token.ok_or(AuthError::Revoked)?;
        let fresh = refresh(client, http, &refresh_token)?;
        self.save(
            account_id,
            &Credential::OAuth {
                tokens: fresh.clone(),
            },
        )?;
        Ok(fresh.access_token)
    }

    /// The saved key that encrypts the local mail database, if there is one.
    /// An entry that is there but unreadable is `Corrupt`.
    pub fn saved_database_key(&self) -> Result<Option<[u8; 32]>, VaultError> {
        match self.store.get(DATABASE_KEY)? {
            Some(hex) => parse_key(&hex).map(Some).ok_or(VaultError::Corrupt),
            None => Ok(None),
        }
    }

    /// Make and save a new database key, replacing any saved one. Only for
    /// a database that doesn't exist yet: a new key can't open old data.
    /// Reads the key back, so a keychain that drops secrets is caught now
    /// rather than after a restart.
    pub fn new_database_key(&self) -> Result<[u8; 32], VaultError> {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|e| SecretError::Other(e.to_string()))?;
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        self.store.set(DATABASE_KEY, &hex)?;
        match self.saved_database_key() {
            Ok(Some(saved)) if saved == key => Ok(key),
            Ok(_) | Err(VaultError::Corrupt) => Err(VaultError::NotKept),
            Err(e) => Err(e),
        }
    }
}

fn parse_key(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::serve;

    fn vault() -> Vault {
        Vault::new(Arc::new(MemoryStore::default()))
    }

    /// Needs a running Secret Service (GNOME Keyring). CI runs it with
    /// `dbus-run-session` and an unlocked keyring.
    #[test]
    #[ignore = "needs a system keychain"]
    fn real_keychain_round_trip() {
        let store = KeyringStore;
        let key = "test:round-trip";
        store.set(key, "hello").expect("set");
        assert_eq!(store.get(key).expect("get").as_deref(), Some("hello"));
        store.delete(key).expect("delete");
        assert_eq!(store.get(key).expect("get"), None);
        store.delete(key).expect("deleting twice is fine");

        let v = Vault::keychain();
        let first = match v.saved_database_key().expect("readable") {
            Some(key) => key,
            None => v.new_database_key().expect("database key"),
        };
        assert_eq!(v.saved_database_key(), Ok(Some(first)), "stable");
    }

    #[test]
    fn credentials_round_trip() {
        let v = vault();
        assert_eq!(v.credential("a"), Err(VaultError::Missing));
        let cred = Credential::Password {
            username: "me@icloud.com".into(),
            password: "abcd-efgh".into(),
        };
        v.save("a", &cred).expect("saved");
        assert_eq!(v.credential("a"), Ok(cred));
        v.forget("a").expect("forgot");
        assert_eq!(v.credential("a"), Err(VaultError::Missing));
    }

    #[test]
    fn database_keys_are_saved_and_read_back() {
        let v = vault();
        assert_eq!(v.saved_database_key(), Ok(None));
        let k1 = v.new_database_key().expect("key");
        assert_ne!(k1, [0u8; 32]);
        assert_eq!(v.saved_database_key(), Ok(Some(k1)));
        let k2 = v.new_database_key().expect("key");
        assert_ne!(k1, k2, "a new key replaces the old one");
        assert_eq!(v.saved_database_key(), Ok(Some(k2)));
    }

    /// Accepts writes, then forgets them, like a wallet that never saved.
    struct Forgetful;

    impl SecretStore for Forgetful {
        fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
            Ok(Some(String::new()))
        }
        fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), SecretError> {
            Ok(())
        }
    }

    #[test]
    fn a_keychain_that_loses_secrets_is_caught() {
        let v = Vault::new(Arc::new(Forgetful));
        assert_eq!(v.new_database_key(), Err(VaultError::NotKept));
        assert_eq!(v.saved_database_key(), Err(VaultError::Corrupt));
    }

    #[test]
    fn expired_tokens_are_refreshed_and_saved() {
        let server = serve(vec![(200, r#"{"access_token":"fresh","expires_in":3600}"#)]);
        let mut client = OAuthClient::google("cid", "sec");
        client.token_url = format!("{}/token", server.url);
        let v = vault();
        v.save(
            "g",
            &Credential::OAuth {
                tokens: Tokens {
                    access_token: "old".into(),
                    refresh_token: Some("rt".into()),
                    expires_at: 1_000,
                },
            },
        )
        .expect("saved");
        let http = Http::new();
        assert_eq!(
            v.access_token("g", &client, &http, 2_000).expect("token"),
            "fresh"
        );
        // Saved, so the next call needs no network.
        let Ok(Credential::OAuth { tokens }) = v.credential("g") else {
            panic!("oauth")
        };
        assert_eq!(tokens.access_token, "fresh");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
    }

    #[test]
    fn valid_tokens_are_used_as_is() {
        let v = vault();
        v.save(
            "g",
            &Credential::OAuth {
                tokens: Tokens {
                    access_token: "good".into(),
                    refresh_token: None,
                    expires_at: 10_000,
                },
            },
        )
        .expect("saved");
        let client = OAuthClient::google("cid", "sec");
        assert_eq!(
            v.access_token("g", &client, &Http::new(), 1_000)
                .expect("token"),
            "good"
        );
    }
}
