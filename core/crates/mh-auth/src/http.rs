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
    /// Sent with every request (Graph's `Prefer` options, for example).
    headers: Vec<(String, String)>,
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
            headers: Vec::new(),
        }
    }

    /// Add a header to every request this client makes.
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
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
        let mut request = self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {token}"));
        for (k, v) in &self.headers {
            request = request.header(k, v);
        }
        let response = request.call().map_err(transport_error)?;
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
        let mut request = match method {
            "PATCH" => self.agent.patch(url),
            "PUT" => self.agent.put(url),
            _ => self.agent.post(url),
        }
        .header("Authorization", &auth);
        for (k, v) in &self.headers {
            request = request.header(k, v);
        }
        let response = request.send_json(body).map_err(transport_error)?;
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
        let mut request = self
            .agent
            .post(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", content_type);
        for (k, v) in &self.headers {
            request = request.header(k, v);
        }
        let response = request.send(body).map_err(transport_error)?;
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
mod tests {
    use super::*;
    use crate::testing::serve;

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
