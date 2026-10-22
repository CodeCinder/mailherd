//! Sign-in and secrets for MailHerd.
//!
//! - [`oauth`]: browser sign-in for Google and Microsoft (OAuth 2.0 + PKCE).
//! - [`secrets`]: credentials and the database key, kept in the OS keychain.
//! - [`http`]: the blocking HTTP client the providers share.

pub mod http;
pub mod oauth;
pub mod secrets;

pub use oauth::{AuthError, OAuthClient, Provider, SignIn, Tokens};
pub use secrets::{Credential, KeyringStore, MemoryStore, SecretStore, Vault, VaultError};

/// MailHerd's Google registration, baked in at build time from the
/// `MH_GOOGLE_CLIENT_ID` and `MH_GOOGLE_CLIENT_SECRET` build secrets.
/// Setting the same variables when running the app overrides them, for testing.
pub fn google_client() -> Option<OAuthClient> {
    let id = std::env::var("MH_GOOGLE_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("MH_GOOGLE_CLIENT_ID").map(String::from))?;
    let secret = std::env::var("MH_GOOGLE_CLIENT_SECRET")
        .ok()
        .or_else(|| option_env!("MH_GOOGLE_CLIENT_SECRET").map(String::from))?;
    (!id.is_empty()).then(|| OAuthClient::google(&id, &secret))
}

/// MailHerd's Microsoft registration, from `MH_MS_CLIENT_ID` and `MH_MS_TENANT_ID`.
pub fn microsoft_client() -> Option<OAuthClient> {
    let id = std::env::var("MH_MS_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("MH_MS_CLIENT_ID").map(String::from))?;
    let tenant = std::env::var("MH_MS_TENANT_ID")
        .ok()
        .or_else(|| option_env!("MH_MS_TENANT_ID").map(String::from))?;
    (!id.is_empty() && !tenant.is_empty()).then(|| OAuthClient::microsoft(&tenant, &id))
}
