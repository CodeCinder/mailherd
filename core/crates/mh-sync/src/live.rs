//! The real services.

use std::time::{SystemTime, UNIX_EPOCH};

use mh_app::sync::{AccessToken, MailProvider, ProviderError};
use mh_auth::http::Http;
use mh_auth::{AuthError, Credential, OAuthClient, Vault, VaultError};
use mh_model::{Account, ProviderKind};
use mh_provider_gmail::Gmail;
use mh_provider_graph::Graph;
use mh_provider_imap::{Imap, ImapConfig};

use crate::Connector;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// What a keychain or token problem means for syncing.
pub(crate) fn vault_error(e: VaultError) -> ProviderError {
    match e {
        VaultError::Missing | VaultError::Corrupt | VaultError::Auth(AuthError::Revoked) => {
            ProviderError::NeedsSignIn
        }
        VaultError::Auth(AuthError::Network(m)) => ProviderError::Retry(m),
        VaultError::Auth(other) => ProviderError::Retry(other.to_string()),
        // A locked keychain, usually; it may be unlocked later.
        VaultError::Secret(e) => ProviderError::Retry(format!("keychain: {e}")),
    }
}

/// Connects to Gmail, Microsoft 365 and iCloud with the credentials in the keychain.
pub struct Live {
    vault: Vault,
    http: Http,
    google: Option<OAuthClient>,
    microsoft: Option<OAuthClient>,
}

impl Live {
    /// Uses the app registrations this build was made with
    /// (see [`mh_auth::google_client`] and [`mh_auth::microsoft_client`]).
    pub fn new(vault: Vault) -> Self {
        Self {
            vault,
            http: Http::new(),
            google: mh_auth::google_client(),
            microsoft: mh_auth::microsoft_client(),
        }
    }

    fn token_source(&self, account: &Account) -> Result<AccessToken, ProviderError> {
        let client = self.oauth_client(account.kind).ok_or_else(|| {
            ProviderError::Failed("this build of MailHerd can't sign in to this account".into())
        })?;
        let (vault, http, id) = (self.vault.clone(), self.http.clone(), account.id.clone());
        Ok(Box::new(move || {
            vault
                .access_token(&id, &client, &http, now())
                .map_err(vault_error)
        }))
    }
}

fn fixed_token(token: &str) -> AccessToken {
    let token = token.to_string();
    Box::new(move || Ok(token.clone()))
}

impl Connector for Live {
    fn connect(&self, account: &Account) -> Result<Box<dyn MailProvider>, ProviderError> {
        let id = account.id.clone();
        Ok(match account.kind {
            ProviderKind::Gmail => Box::new(Gmail::new(id, self.token_source(account)?)),
            ProviderKind::Microsoft365 => Box::new(Graph::new(id, self.token_source(account)?)),
            ProviderKind::ICloud => match self.vault.credential(&account.id) {
                Ok(Credential::Password { username, password }) => {
                    Box::new(Imap::new(id, ImapConfig::icloud(&username, &password)))
                }
                Ok(Credential::OAuth { .. }) => return Err(ProviderError::NeedsSignIn),
                Err(e) => return Err(vault_error(e)),
            },
        })
    }

    fn whoami(&self, kind: ProviderKind, access_token: &str) -> Result<String, ProviderError> {
        match kind {
            ProviderKind::Gmail => {
                Gmail::new(String::new(), fixed_token(access_token)).email_address()
            }
            ProviderKind::Microsoft365 => {
                Graph::new(String::new(), fixed_token(access_token)).email_address()
            }
            ProviderKind::ICloud => Err(ProviderError::Failed(
                "iCloud doesn't use browser sign-in".into(),
            )),
        }
    }

    fn check_password(&self, email: &str, password: &str) -> Result<(), ProviderError> {
        Imap::check_login(&ImapConfig::icloud(email, password))
    }

    fn oauth_client(&self, kind: ProviderKind) -> Option<OAuthClient> {
        match kind {
            ProviderKind::Gmail => self.google.clone(),
            ProviderKind::Microsoft365 => self.microsoft.clone(),
            ProviderKind::ICloud => None,
        }
    }

    fn vault(&self) -> &Vault {
        &self.vault
    }

    fn renew(&self, account: &Account) {
        if let Ok(Credential::OAuth { mut tokens }) = self.vault.credential(&account.id) {
            tokens.expires_at = 0;
            let _ = self.vault.save(&account.id, &Credential::OAuth { tokens });
        }
    }
}
