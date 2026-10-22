//! A small blocking HTTP client shared by the sign-in code and the providers.
//! Every call runs on a background thread, never on the UI thread.

use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Largest response body accepted, in bytes. Raw messages with big attachments
/// can be tens of megabytes.
const MAX_BODY: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HttpError {
    /// The access token was rejected (HTTP 401).
    #[error("not authorised")]
    Unauthorized,
    /// Rate limited, a server error, or a network problem: try again later.
    #[error("temporary problem: {0}")]
    Retry(String),
    /// Any other failure; retrying the same request won't help.
    #[error("request failed: {status} {body}")]
    Failed { status: u16, body: String },
    #[error("unexpected response: {0}")]
    Decode(String),
}

pub type HttpResult<T> = Result<T, HttpError>;

#[derive(Clone)]
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

fn classify(status: u16, body: String) -> HttpError {
    match status {
        401 => HttpError::Unauthorized,
        408 | 429 | 500..=599 => HttpError::Retry(format!("HTTP {status}")),
        _ => HttpError::Failed {
            status,
            body: body.chars().take(500).collect(),
        },
    }
}

fn transport_error(e: ureq::Error) -> HttpError {
    HttpError::Retry(e.to_string())
}

impl Http {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(120)))
            .user_agent(concat!("MailHerd/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
        }
    }

    fn finish(mut response: ureq::http::Response<ureq::Body>) -> HttpResult<Vec<u8>> {
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(transport_error)?;
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(classify(
                status,
                String::from_utf8_lossy(&body).into_owned(),
            ))
        }
    }

    fn decode<T: DeserializeOwned>(bytes: &[u8]) -> HttpResult<T> {
        if bytes.is_empty() {
            return serde_json::from_str("null").map_err(|e| HttpError::Decode(e.to_string()));
        }
        serde_json::from_slice(bytes).map_err(|e| HttpError::Decode(e.to_string()))
    }

    /// GET with a bearer token, returning the raw body.
    pub fn get_bytes(&self, url: &str, token: &str) -> HttpResult<Vec<u8>> {
        let response = self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(transport_error)?;
        Self::finish(response)
    }

    pub fn get_json<T: DeserializeOwned>(&self, url: &str, token: &str) -> HttpResult<T> {
        Self::decode(&self.get_bytes(url, token)?)
    }

    /// POST or PATCH a JSON body with a bearer token.
    pub fn send_json<B: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        url: &str,
        token: &str,
        body: &B,
    ) -> HttpResult<T> {
        let auth = format!("Bearer {token}");
        let response = match method {
            "PATCH" => self
                .agent
                .patch(url)
                .header("Authorization", &auth)
                .send_json(body),
            "PUT" => self
                .agent
                .put(url)
                .header("Authorization", &auth)
                .send_json(body),
            _ => self
                .agent
                .post(url)
                .header("Authorization", &auth)
                .send_json(body),
        }
        .map_err(transport_error)?;
        Self::decode(&Self::finish(response)?)
    }

    /// POST a raw body with a content type and bearer token.
    pub fn post_raw(
        &self,
        url: &str,
        token: &str,
        content_type: &str,
        body: &[u8],
    ) -> HttpResult<Vec<u8>> {
        let response = self
            .agent
            .post(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", content_type)
            .send(body)
            .map_err(transport_error)?;
        Self::finish(response)
    }

    /// POST a form without a token (used for the OAuth token endpoint).
    pub fn post_form<T: DeserializeOwned>(
        &self,
        url: &str,
        form: &[(&str, &str)],
    ) -> HttpResult<T> {
        let response = self
            .agent
            .post(url)
            .send_form(form.iter().copied())
            .map_err(transport_error)?;
        Self::decode(&Self::finish(response)?)
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! A tiny HTTP server for tests: answers each request with the next canned
    //! response and records what it received.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[derive(Debug, Clone)]
    pub struct Recorded {
        pub request_line: String,
        pub headers: Vec<String>,
        pub body: String,
    }

    pub struct MockServer {
        pub url: String,
        pub requests: Arc<Mutex<Vec<Recorded>>>,
    }

    pub fn serve(responses: Vec<(u16, &'static str)>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut request_line = String::new();
                let _ = reader.read_line(&mut request_line);
                let mut headers = Vec::new();
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    headers.push(line.trim_end().to_string());
                }
                let mut buf = vec![0u8; length];
                let _ = reader.read_exact(&mut buf);
                log.lock().expect("lock").push(Recorded {
                    request_line: request_line.trim_end().to_string(),
                    headers,
                    body: String::from_utf8_lossy(&buf).into_owned(),
                });
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        MockServer { url, requests }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::serve;
    use super::*;

    #[test]
    fn decodes_json_and_sends_the_token() {
        let server = serve(vec![(200, r#"{"ok":true}"#)]);
        let v: serde_json::Value = Http::new()
            .get_json(&format!("{}/x", server.url), "tok")
            .expect("ok");
        assert_eq!(v["ok"], true);
        let req = &server.requests.lock().expect("lock")[0];
        assert!(
            req.headers
                .iter()
                .any(|h| h == "authorization: Bearer tok" || h == "Authorization: Bearer tok")
        );
    }

    #[test]
    fn classifies_errors() {
        let server = serve(vec![(401, "{}"), (429, "{}"), (400, r#"{"error":"bad"}"#)]);
        let http = Http::new();
        let url = format!("{}/x", server.url);
        assert_eq!(http.get_bytes(&url, "t"), Err(HttpError::Unauthorized));
        assert!(matches!(
            http.get_bytes(&url, "t"),
            Err(HttpError::Retry(_))
        ));
        assert!(matches!(
            http.get_bytes(&url, "t"),
            Err(HttpError::Failed { status: 400, .. })
        ));
    }
}
