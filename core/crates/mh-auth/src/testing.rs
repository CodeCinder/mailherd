//! Fake HTTP servers for tests.
//!
//! [`serve`] answers requests with canned responses in order. [`MockApi`]
//! routes by method and path, which suits provider tests that make many calls.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    /// Path and query, e.g. `/gmail/v1/users/me/messages?q=x`.
    pub target: String,
    pub request_line: String,
    pub headers: Vec<String>,
    pub body: String,
}

fn read_request(stream: &TcpStream) -> Option<Recorded> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).ok()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
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
    reader.read_exact(&mut buf).ok()?;
    Some(Recorded {
        method,
        target,
        request_line: request_line.trim_end().to_string(),
        headers,
        body: String::from_utf8_lossy(&buf).into_owned(),
    })
}

fn reply(stream: &mut TcpStream, status: u16, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

pub struct MockServer {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
}

/// Answer one request per canned response, in order.
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
            if let Some(r) = read_request(&stream) {
                log.lock().expect("lock").push(r);
            }
            reply(&mut stream, status, body.as_bytes());
        }
    });
    MockServer { url, requests }
}

struct Route {
    method: String,
    path_prefix: String,
    query_contains: Option<String>,
    status: u16,
    body: Vec<u8>,
    once: bool,
    used: bool,
}

/// A fake REST API. Routes are matched in the order they were added; a route
/// added with `once` answers a single time, then later routes get a turn.
pub struct MockApi {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
    routes: Arc<Mutex<Vec<Route>>>,
    stop: Arc<AtomicBool>,
}

impl MockApi {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let routes: Arc<Mutex<Vec<Route>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (log, table, halt) = (
            Arc::clone(&requests),
            Arc::clone(&routes),
            Arc::clone(&stop),
        );
        thread::spawn(move || {
            while !halt.load(Ordering::SeqCst) {
                let mut stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(_) => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                };
                let _ = stream.set_nonblocking(false);
                let Some(req) = read_request(&stream) else {
                    continue;
                };
                let (path, query) = match req.target.split_once('?') {
                    Some((p, q)) => (p.to_string(), q.to_string()),
                    None => (req.target.clone(), String::new()),
                };
                let found = {
                    let mut routes = table.lock().expect("lock");
                    routes
                        .iter_mut()
                        .find(|r| {
                            !(r.once && r.used)
                                && r.method == req.method
                                && path.starts_with(&r.path_prefix)
                                && r.query_contains
                                    .as_ref()
                                    .is_none_or(|q| query.contains(q.as_str()))
                        })
                        .map(|r| {
                            r.used = true;
                            (r.status, r.body.clone())
                        })
                };
                log.lock().expect("lock").push(req);
                match found {
                    Some((status, body)) => reply(&mut stream, status, &body),
                    None => reply(&mut stream, 404, br#"{"error":"no mock route"}"#),
                }
            }
        });
        Self {
            url,
            requests,
            routes,
            stop,
        }
    }

    fn add(
        &self,
        method: &str,
        path: &str,
        query: Option<&str>,
        status: u16,
        body: &[u8],
        once: bool,
    ) -> &Self {
        self.routes.lock().expect("lock").push(Route {
            method: method.to_string(),
            path_prefix: path.to_string(),
            query_contains: query.map(String::from),
            status,
            body: body.to_vec(),
            once,
            used: false,
        });
        self
    }

    pub fn on(&self, method: &str, path: &str, status: u16, body: &str) -> &Self {
        self.add(method, path, None, status, body.as_bytes(), false)
    }

    pub fn on_query(
        &self,
        method: &str,
        path: &str,
        query: &str,
        status: u16,
        body: &str,
    ) -> &Self {
        self.add(method, path, Some(query), status, body.as_bytes(), false)
    }

    pub fn once(&self, method: &str, path: &str, status: u16, body: &str) -> &Self {
        self.add(method, path, None, status, body.as_bytes(), true)
    }

    pub fn on_bytes(&self, method: &str, path: &str, status: u16, body: &[u8]) -> &Self {
        self.add(method, path, None, status, body, false)
    }

    pub fn requests_to(&self, method: &str, path: &str) -> Vec<Recorded> {
        self.requests
            .lock()
            .expect("lock")
            .iter()
            .filter(|r| r.method == method && r.target.starts_with(path))
            .cloned()
            .collect()
    }
}

impl Drop for MockApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}
