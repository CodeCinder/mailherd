//! Signing in with Google and Microsoft: OAuth 2.0 authorization code flow
//! with PKCE and a loopback redirect, as recommended for desktop apps.
//!
//! The app opens the provider's sign-in page in the browser. After the user
//! signs in, the provider redirects the browser to `http://127.0.0.1:<port>`,
//! where [`SignIn::finish`] is listening for the one-time code.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::http::{Http, HttpError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Google,
    Microsoft,
}

/// An app registration with one provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthClient {
    pub provider: Provider,
    pub client_id: String,
    /// Google issues a secret even for desktop apps; it isn't confidential.
    pub client_secret: Option<String>,
    pub auth_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    /// Host used in the redirect URI. Must match the registration.
    pub redirect_host: &'static str,
    pub extra_auth_params: Vec<(String, String)>,
}

const GOOGLE_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.modify",
    "https://www.googleapis.com/auth/gmail.settings.basic",
    "https://www.googleapis.com/auth/contacts.readonly",
    "https://www.googleapis.com/auth/calendar",
    "https://www.googleapis.com/auth/drive.file",
];

const MICROSOFT_SCOPES: &[&str] = &[
    "offline_access",
    "openid",
    "profile",
    "email",
    "https://graph.microsoft.com/User.Read",
    "https://graph.microsoft.com/Mail.ReadWrite",
    "https://graph.microsoft.com/Mail.Send",
    "https://graph.microsoft.com/MailboxSettings.ReadWrite",
    "https://graph.microsoft.com/Contacts.Read",
    "https://graph.microsoft.com/Calendars.ReadWrite",
    "https://graph.microsoft.com/Files.ReadWrite",
];

impl OAuthClient {
    pub fn google(client_id: &str, client_secret: &str) -> Self {
        Self {
            provider: Provider::Google,
            client_id: client_id.to_string(),
            client_secret: Some(client_secret.to_string()),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token_url: "https://oauth2.googleapis.com/token".into(),
            scopes: GOOGLE_SCOPES.iter().map(|s| s.to_string()).collect(),
            redirect_host: "127.0.0.1",
            // Ask for a refresh token every time so a re-sign-in always gets one.
            extra_auth_params: vec![
                ("access_type".into(), "offline".into()),
                ("prompt".into(), "consent".into()),
            ],
        }
    }

    pub fn microsoft(tenant_id: &str, client_id: &str) -> Self {
        let base = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0");
        Self {
            provider: Provider::Microsoft,
            client_id: client_id.to_string(),
            client_secret: None,
            auth_url: format!("{base}/authorize"),
            token_url: format!("{base}/token"),
            scopes: MICROSOFT_SCOPES.iter().map(|s| s.to_string()).collect(),
            redirect_host: "localhost",
            extra_auth_params: vec![("prompt".into(), "select_account".into())],
        }
    }
}

/// Tokens from a sign-in or refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Seconds since the epoch when the access token expires.
    pub expires_at: i64,
}

impl Tokens {
    /// True if the access token expires within the next minute.
    pub fn expiring(&self, now: i64) -> bool {
        self.expires_at - now < 60
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("sign-in was cancelled or denied: {0}")]
    Denied(String),
    #[error("sign-in timed out")]
    TimedOut,
    #[error("the sign-in response didn't match this request")]
    StateMismatch,
    /// The saved sign-in no longer works (password changed, access revoked).
    #[error("the saved sign-in is no longer valid")]
    Revoked,
    #[error("could not reach the sign-in server: {0}")]
    Network(String),
    #[error("local sign-in listener failed: {0}")]
    Io(String),
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn random_token(bytes: usize) -> Result<String, AuthError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| AuthError::Io(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// The PKCE pair: the challenge goes in the sign-in URL, the verifier proves
/// it was us when exchanging the code.
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A sign-in in progress: open [`SignIn::url`] in the browser, then call [`SignIn::finish`].
pub struct SignIn {
    client: OAuthClient,
    url: String,
    redirect_uri: String,
    state: String,
    verifier: String,
    listener: TcpListener,
}

impl SignIn {
    pub fn start(client: &OAuthClient) -> Result<Self, AuthError> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|e| AuthError::Io(e.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|e| AuthError::Io(e.to_string()))?
            .port();
        let redirect_uri = format!("http://{}:{port}", client.redirect_host);
        let state = random_token(16)?;
        let verifier = random_token(32)?;

        let mut url = Url::parse(&client.auth_url).map_err(|e| AuthError::Io(e.to_string()))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("client_id", &client.client_id)
                .append_pair("redirect_uri", &redirect_uri)
                .append_pair("response_type", "code")
                .append_pair("scope", &client.scopes.join(" "))
                .append_pair("state", &state)
                .append_pair("code_challenge", &pkce_challenge(&verifier))
                .append_pair("code_challenge_method", "S256");
            for (k, v) in &client.extra_auth_params {
                q.append_pair(k, v);
            }
        }
        Ok(Self {
            client: client.clone(),
            url: url.to_string(),
            redirect_uri,
            state,
            verifier,
            listener,
        })
    }

    /// The page to open in the user's browser.
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Wait for the browser to come back, then exchange the code for tokens.
    pub fn finish(self, timeout: Duration, http: &Http) -> Result<Tokens, AuthError> {
        let code = self.wait_for_code(timeout)?;
        exchange_code(
            &self.client,
            http,
            &code,
            &self.verifier,
            &self.redirect_uri,
        )
    }

    fn wait_for_code(&self, timeout: Duration) -> Result<String, AuthError> {
        self.listener
            .set_nonblocking(true)
            .map_err(|e| AuthError::Io(e.to_string()))?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(result) = self.handle(stream) {
                        return result;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(AuthError::TimedOut);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(AuthError::Io(e.to_string())),
            }
        }
    }

    /// Handle one browser request. `None` means "not the redirect" (a favicon
    /// request, say) and we keep waiting.
    fn handle(&self, mut stream: TcpStream) -> Option<Result<String, AuthError>> {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).ok()?;
        let target = line.split_whitespace().nth(1)?;
        let url = Url::parse(&format!("http://localhost{target}")).ok()?;
        let param = |name: &str| {
            url.query_pairs()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.into_owned())
        };
        let (result, page) = match (param("code"), param("error")) {
            (Some(code), _) if param("state").as_deref() == Some(self.state.as_str()) => (
                Ok(code),
                "You're signed in. You can close this tab and go back to MailHerd.",
            ),
            (Some(_), _) => (
                Err(AuthError::StateMismatch),
                "Something went wrong. Please try again from MailHerd.",
            ),
            (None, Some(err)) => {
                let detail = param("error_description").unwrap_or(err);
                (
                    Err(AuthError::Denied(detail)),
                    "Sign-in was cancelled. You can close this tab.",
                )
            }
            (None, None) => {
                let _ = write!(
                    stream,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                return None;
            }
        };
        let html = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>MailHerd</title></head>\
             <body style=\"font-family:sans-serif;text-align:center;margin-top:20vh\">\
             <h1>MailHerd</h1><p>{page}</p></body></html>"
        );
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        Some(result)
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

fn token_error(e: HttpError) -> AuthError {
    match e {
        // invalid_grant: the refresh token was revoked or expired.
        HttpError::Failed {
            status: 400,
            ref body,
        } if body.contains("invalid_grant") => AuthError::Revoked,
        HttpError::Failed { status, body } => AuthError::Denied(format!("{status} {body}")),
        HttpError::Unauthorized => AuthError::Revoked,
        other => AuthError::Network(other.to_string()),
    }
}

fn token_request(
    client: &OAuthClient,
    http: &Http,
    form: &mut Vec<(&str, String)>,
) -> Result<TokenResponse, AuthError> {
    form.push(("client_id", client.client_id.clone()));
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    http.post_form(&client.token_url, &pairs)
        .map_err(token_error)
}

pub fn exchange_code(
    client: &OAuthClient,
    http: &Http,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Tokens, AuthError> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.to_string()),
        ("code_verifier", verifier.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
    ];
    let t = token_request(client, http, &mut form)?;
    Ok(Tokens {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at: now() + t.expires_in.unwrap_or(3600),
    })
}

/// Get a fresh access token. Keeps the old refresh token when the provider
/// doesn't issue a new one.
pub fn refresh(
    client: &OAuthClient,
    http: &Http,
    refresh_token: &str,
) -> Result<Tokens, AuthError> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];
    if client.provider == Provider::Microsoft {
        form.push(("scope", client.scopes.join(" ")));
    }
    let t = token_request(client, http, &mut form)?;
    Ok(Tokens {
        access_token: t.access_token,
        refresh_token: t.refresh_token.or_else(|| Some(refresh_token.to_string())),
        expires_at: now() + t.expires_in.unwrap_or(3600),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::testing::serve;

    fn test_client(token_url: &str) -> OAuthClient {
        let mut c = OAuthClient::google("cid.apps.googleusercontent.com", "sec");
        c.token_url = token_url.to_string();
        c
    }

    #[test]
    fn pkce_matches_the_rfc_example() {
        // RFC 7636 appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn sign_in_url_has_everything_the_provider_needs() {
        let signin = SignIn::start(&OAuthClient::microsoft("tenant-1", "client-1")).expect("start");
        let url = Url::parse(signin.url()).expect("url");
        assert_eq!(url.path(), "/tenant-1/oauth2/v2.0/authorize");
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "client-1");
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(q["redirect_uri"].starts_with("http://localhost:"));
        assert!(q["scope"].contains("offline_access"));
        assert!(q["scope"].contains("https://graph.microsoft.com/Mail.Send"));
    }

    fn browser_get(redirect_uri: &str, path: &str) -> String {
        let addr = redirect_uri
            .replace("http://localhost", "127.0.0.1")
            .replace("http://", "");
        let mut s = TcpStream::connect(addr).expect("connect");
        write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").expect("write");
        let mut reply = String::new();
        let _ = std::io::Read::read_to_string(&mut s, &mut reply);
        reply
    }

    fn browser_redirect(redirect_uri: &str, query: &str) {
        assert!(browser_get(redirect_uri, &format!("/?{query}")).contains("MailHerd"));
    }

    #[test]
    fn full_sign_in_exchanges_the_code() {
        let server = serve(vec![(
            200,
            r#"{"access_token":"at","refresh_token":"rt","expires_in":3599}"#,
        )]);
        let client = test_client(&format!("{}/token", server.url));
        let signin = SignIn::start(&client).expect("start");
        let state = Url::parse(signin.url())
            .expect("url")
            .query_pairs()
            .find(|(k, _)| k == "state")
            .map(|(_, v)| v.into_owned())
            .expect("state");
        let redirect = signin.redirect_uri().to_string();
        let browser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            assert!(browser_get(&redirect, "/favicon.ico").starts_with("HTTP/1.1 404"));
            browser_redirect(&redirect, &format!("code=abc&state={state}"));
        });
        let tokens = signin
            .finish(Duration::from_secs(10), &Http::new())
            .expect("tokens");
        browser.join().expect("browser");
        assert_eq!(tokens.access_token, "at");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        let request = &server.requests.lock().expect("lock")[0];
        assert!(request.request_line.starts_with("POST /token"));
        let body = &request.body;
        assert!(body.contains("grant_type=authorization_code"));
        assert!(body.contains("code=abc"));
        assert!(body.contains("code_verifier="));
        assert!(body.contains("client_secret=sec"));
    }

    #[test]
    fn a_wrong_state_is_rejected() {
        let client = test_client("http://127.0.0.1:9/token");
        let signin = SignIn::start(&client).expect("start");
        let redirect = signin.redirect_uri().to_string();
        let browser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            browser_redirect(&redirect, "code=abc&state=forged");
        });
        assert_eq!(
            signin.finish(Duration::from_secs(10), &Http::new()),
            Err(AuthError::StateMismatch)
        );
        browser.join().expect("browser");
    }

    #[test]
    fn cancelling_in_the_browser_is_reported() {
        let signin = SignIn::start(&test_client("http://127.0.0.1:9/token")).expect("start");
        let redirect = signin.redirect_uri().to_string();
        let browser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            browser_redirect(
                &redirect,
                "error=access_denied&error_description=User+said+no",
            );
        });
        assert_eq!(
            signin.finish(Duration::from_secs(10), &Http::new()),
            Err(AuthError::Denied("User said no".into()))
        );
        browser.join().expect("browser");
    }

    #[test]
    fn times_out_when_the_browser_never_comes_back() {
        let signin = SignIn::start(&test_client("http://127.0.0.1:9/token")).expect("start");
        assert_eq!(
            signin.finish(Duration::from_millis(300), &Http::new()),
            Err(AuthError::TimedOut)
        );
    }

    #[test]
    fn refresh_keeps_the_old_refresh_token_and_detects_revocation() {
        let server = serve(vec![
            (200, r#"{"access_token":"new","expires_in":3600}"#),
            (
                400,
                r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
            ),
        ]);
        let client = test_client(&format!("{}/token", server.url));
        let http = Http::new();
        let t = refresh(&client, &http, "rt1").expect("refreshed");
        assert_eq!(t.access_token, "new");
        assert_eq!(t.refresh_token.as_deref(), Some("rt1"));
        assert_eq!(refresh(&client, &http, "rt1"), Err(AuthError::Revoked));
    }
}
