//! A mock HTTP server for the router's tests.
//!
//! `std::net::TcpListener`, one thread, and a script of answers. The
//! router's tests never touch the network and never read the real secrets
//! file: they read a temp file this module writes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// One scripted answer.
#[derive(Clone, Debug)]
pub struct MockResponse {
    /// The status to answer with.
    pub status: u16,
    /// The body to answer with.
    pub body: String,
    /// How long to wait before answering.
    pub delay_ms: u64,
}

impl MockResponse {
    /// A 200 answer.
    pub fn ok(body: impl Into<String>) -> MockResponse {
        MockResponse {
            status: 200,
            body: body.into(),
            delay_ms: 0,
        }
    }

    /// An answer with a status.
    pub fn status(status: u16, body: impl Into<String>) -> MockResponse {
        MockResponse {
            status,
            body: body.into(),
            delay_ms: 0,
        }
    }

    /// A 200 answer that comes late.
    pub fn slow(body: impl Into<String>, delay_ms: u64) -> MockResponse {
        MockResponse {
            status: 200,
            body: body.into(),
            delay_ms,
        }
    }
}

/// A request the server received.
#[derive(Clone, Debug)]
pub struct MockRequest {
    /// The method.
    pub method: String,
    /// The path.
    pub path: String,
    /// The headers, as they came in.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: String,
}

impl MockRequest {
    /// The value of a header, looked up without regard to case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The body, as JSON.
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// A mock server: it answers with the scripted responses, in order.
pub struct MockServer {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<MockRequest>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockServer {
    /// Start a server that answers with `responses`, in order.
    pub fn start(responses: Vec<MockResponse>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the mock server binds");
        let addr = listener
            .local_addr()
            .expect("the mock server has an address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let script = Arc::new(Mutex::new(responses.into_iter().collect::<VecDeque<_>>()));

        let thread_requests = Arc::clone(&requests);
        let thread_stop = Arc::clone(&stop);
        let thread_script = Arc::clone(&script);
        let handle = thread::spawn(move || {
            for stream in listener.incoming() {
                if thread_stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if let Some(request) = read_request(&stream) {
                    thread_requests
                        .lock()
                        .expect("the request log")
                        .push(request);
                }
                let response = thread_script.lock().expect("the script").pop_front();
                let response = response.unwrap_or_else(|| {
                    MockResponse::status(500, "the mock server has no answer left for this request")
                });
                let _ = write_response(&stream, &response);
            }
        });

        MockServer {
            addr,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    /// The base URL of the server.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Every request the server received.
    pub fn requests(&self) -> Vec<MockRequest> {
        self.requests.lock().expect("the request log").clone()
    }

    /// How many requests the server received.
    pub fn count(&self) -> usize {
        self.requests.lock().expect("the request log").len()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop so the thread can end.
        let _ = TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Read one HTTP request: the head, then the body it says there is.
fn read_request(stream: &TcpStream) -> Option<MockRequest> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone().ok()?);

    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut head = String::new();
        if reader.read_line(&mut head).ok()? == 0 {
            break;
        }
        let head = head.trim_end();
        if head.is_empty() {
            break;
        }
        if let Some((name, value)) = head.split_once(':') {
            let name = name.trim().to_string();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((name, value));
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 && reader.read_exact(&mut body).is_err() {
        return None;
    }

    Some(MockRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

/// Write one HTTP response and close the connection.
fn write_response(stream: &TcpStream, response: &MockResponse) -> std::io::Result<()> {
    if response.delay_ms > 0 {
        thread::sleep(Duration::from_millis(response.delay_ms));
    }
    let mut out = stream.try_clone()?;
    let body = response.body.as_bytes();
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        reason(response.status),
        body.len()
    );
    out.write_all(head.as_bytes())?;
    out.write_all(body)?;
    out.flush()
}

/// The reason phrase of a status, for the mock's response line.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Status",
    }
}

/// Write a temp secrets file with these keys, and return its path.
///
/// The file is the only secrets file the tests ever read: the real one at
/// `~/.config/last-mile/secrets.env` is never touched.
pub fn secrets_file(tag: &str, keys: &[(&str, &str)]) -> PathBuf {
    let path = std::env::temp_dir().join(format!("lm-router-{}-{}.env", tag, std::process::id()));
    let mut text = String::new();
    for (name, value) in keys {
        text.push_str(name);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
    std::fs::write(&path, text).expect("the temp secrets file is written");
    path
}

/// The path of a temp secrets file that does not exist.
pub fn missing_secrets_file(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "lm-router-missing-{}-{}.env",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

/// A config with two OpenAI-shaped pools, pointing at the given base URLs.
pub fn two_pool_config(
    secrets_path: &Path,
    zai_base: &str,
    minimax_base: &str,
    zai_requests: u64,
) -> String {
    format!(
        r#"
[router]
secrets_path = "{secrets}"
window_seconds = 3600

[pools.zai]
kind = "openai"
base_url = "{zai_base}"
model = "glm-test"
key_name = "ZAI_API_KEY"
timeout_ms = 2000
cooldown_ms = 600000

[pools.zai.allotment]
tokens = 100000
requests = {zai_requests}

[pools.minimax]
kind = "openai"
base_url = "{minimax_base}"
model = "minimax-test"
key_name = "MINIMAX_API_KEY"
timeout_ms = 2000
cooldown_ms = 600000

[pools.minimax.allotment]
tokens = 100000
requests = 100

[exchange]
zai = 2
minimax = 1
"#,
        secrets = secrets_path.display(),
        zai_base = zai_base,
        minimax_base = minimax_base,
        zai_requests = zai_requests,
    )
}
