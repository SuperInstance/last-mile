//! A mock skald daemon for the tests: a loopback HTTP server that answers
//! exactly what the test scripted.
//!
//! `cargo test` never touches a network: the bank, the queue and the
//! client are proven against this server on `127.0.0.1`, which is a
//! different process on the same machine and not a service anywhere else.
//! The live daemon keeps its own lane — the `#[ignore]`d tests — exactly
//! as `scripts/live.sh` separates the Thor's facts from the build's.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

/// What the mock answers a request with.
#[derive(Clone, Debug)]
pub enum Plan {
    /// HTTP 200 and these bytes as `audio/wav`.
    Wav(Vec<u8>),
    /// This status and this body: the failure a test wants to see named.
    Status(u16, String),
}

/// The mock's shared state: the plan, the requests it has seen, and the
/// gate a test can close to hold a response back.
#[derive(Debug)]
struct Ctl {
    plan: Plan,
    seen: Vec<String>,
    /// While true, every response is held at the door.
    hold: bool,
    /// The accept loop leaves when this is set.
    stopped: bool,
}

/// A mock skald daemon on a loopback port of its own.
pub struct MockSkald {
    port: u16,
    ctl: Arc<(Mutex<Ctl>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl MockSkald {
    /// Start a mock that answers every request with `plan`.
    pub fn start(plan: Plan) -> MockSkald {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
        let port = listener.local_addr().expect("the port is known").port();
        let ctl = Arc::new((
            Mutex::new(Ctl {
                plan,
                seen: Vec::new(),
                hold: false,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let server_ctl = Arc::clone(&ctl);
        let handle = std::thread::spawn(move || accept_loop(listener, server_ctl));
        MockSkald {
            port,
            ctl,
            handle: Some(handle),
        }
    }

    /// The mock's host, as a client's base URL spells it.
    pub fn base(&self) -> String {
        "http://127.0.0.1".to_owned()
    }

    /// The mock's port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The bodies of the requests the mock has been posted, in order.
    pub fn requests(&self) -> Vec<String> {
        self.ctl
            .0
            .lock()
            .map(|ctl| ctl.seen.clone())
            .unwrap_or_default()
    }

    /// Hold every response at the door until [`MockSkald::release`].
    pub fn hold(&self) {
        if let Ok(mut ctl) = self.ctl.0.lock() {
            ctl.hold = true;
        }
    }

    /// Let the held responses through.
    pub fn release(&self) {
        if let Ok(mut ctl) = self.ctl.0.lock() {
            ctl.hold = false;
        }
        self.ctl.1.notify_all();
    }

    /// Stop the mock and wait for its threads to leave.
    pub fn join(mut self) {
        if let Ok(mut ctl) = self.ctl.0.lock() {
            ctl.stopped = true;
            ctl.hold = false;
        }
        self.ctl.1.notify_all();
        // Wake the accept loop: it is blocked in accept() and only a
        // connection of its own will show it the door.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// The accept loop: one thread a connection, so a held response never
/// blocks the next test's call.
fn accept_loop(listener: TcpListener, ctl: Arc<(Mutex<Ctl>, Condvar)>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { break };
        {
            let (lock, _) = &*ctl;
            if let Ok(guard) = lock.lock() {
                if guard.stopped {
                    break;
                }
            }
        }
        let ctl = Arc::clone(&ctl);
        std::thread::spawn(move || serve(stream, ctl));
    }
}

/// Serve one request: read it, remember it, and answer per the plan —
/// once the gate, if it is closed, is opened again.
fn serve(mut stream: TcpStream, ctl: Arc<(Mutex<Ctl>, Condvar)>) {
    let Some(body) = read_request(&mut stream) else {
        return;
    };
    {
        let (lock, _) = &*ctl;
        if let Ok(mut guard) = lock.lock() {
            guard.seen.push(body);
        }
    }
    // The gate: a test that wants to see `pending` or a timeout closes it.
    {
        let (lock, cvar) = &*ctl;
        if let Ok(mut guard) = lock.lock() {
            while guard.hold && !guard.stopped {
                guard = match cvar.wait(guard) {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
            }
            if guard.stopped {
                return;
            }
        }
    }
    let answer = {
        let (lock, _) = &*ctl;
        lock.lock()
            .map(|guard| match &guard.plan {
                Plan::Wav(wav) => response(200, "audio/wav", wav),
                Plan::Status(code, body) => response(*code, "text/plain", body.as_bytes()),
            })
            .unwrap_or_else(|_| response(500, "text/plain", b"mock broken"))
    };
    let _ = stream.write_all(&answer);
    let _ = stream.flush();
}

/// Read one HTTP request off the wire and return its body.
fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // Until the end of the headers.
    let header_end = loop {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = find(&buf, b"\r\n\r\n") {
            break at + 4;
        }
        if buf.len() > 64 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("content-length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while buf.len() < header_end + length {
        let n = stream.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Some(String::from_utf8_lossy(&buf[header_end..header_end + length]).to_string())
}

/// An HTTP response with `Connection: close`, so the client and the mock
/// never argue about whose connection it was.
fn response(code: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Whatever",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    out
}

/// The first place `needle` appears in `hay`, if it appears.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
