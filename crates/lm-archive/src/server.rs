//! The service: the accept loop, the connections, and the verification worker.
//!
//! The archive is the third node of the M1 loop, and the Orin is a box that
//! belongs to somebody else. Three properties shape the service:
//!
//! - **The accept loop never waits.** A connection is accepted, greeted and
//!   handed to its own thread; the loop goes straight back to the listener.
//!   A push that takes a minute to verify must not stop the next push from
//!   arriving (spec §8: the Orin must never become the bottleneck that
//!   stalls the ProArt).
//! - **The verification is a separate thread with a bounded queue.** The
//!   replay is the expensive part of a push, so it happens on a worker that
//!   takes its rounds from a queue of a fixed size. A full queue is
//!   backpressure — the connection waits — and never the accept loop.
//! - **A stalled peer cannot pin a thread.** Every connection gets a read
//!   timeout and `TCP_NODELAY`, so a peer that opens a socket and says
//!   nothing is dropped rather than parked on a thread forever.
//!
//! ## The shape of a push
//!
//! ```text
//! peer                         the service
//! ----                         -----------
//! connect -------------------> accepted, greeted with a Hello
//! Hello, Header, Stamped... -> read, rebuilt into a round
//! Bye -----------------------> the round is stored, atomically
//!                             the round is queued for the worker
//!                             the worker replays it and writes the report
//! verdict <------------------ StateHash, or a Bye that says why not
//! ```
//!
//! The verdict still goes back over the same socket: the ProArt's HUD shows
//! what the Orin found. What changed is *when* it is computed — on the
//! worker, not on the thread that read the push.

use std::io::{self, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use lm_core::log::RoundLog;
use lm_proto::{Msg, NodeKind};

use crate::protocol::{self, IngestError, IngestReport};
use crate::verify::{self, VerifyReport};
use crate::ArchiveConfig;

/// How long the accept loop waits between polls of a quiet listener.
///
/// The listener is polled rather than blocked on, so the service can stop
/// when it is asked to. A few milliseconds costs nothing and keeps the
/// answer to a new connection prompt.
const ACCEPT_POLL: Duration = Duration::from_millis(20);

/// How long a worker waits for a round before it looks at the stop flag.
const WORKER_POLL: Duration = Duration::from_millis(100);

/// How a round is verified.
///
/// The default is [`verify::verify_log`]. A caller may install its own —
/// the tests do, to make a verification slow on purpose and measure that the
/// accept loop does not wait for it.
pub type Verifier = Arc<dyn Fn(&RoundLog) -> VerifyReport + Send + Sync>;

/// A round waiting to be verified.
struct Job {
    /// The round to replay.
    log: RoundLog,
    /// The name the round was stored under.
    name: String,
    /// Where the verdict goes: the connection's thread is waiting on it.
    reply: Sender<VerifyReport>,
}

/// The counters the service keeps about itself.
///
/// They are the service's own bookkeeping — the store keeps the rounds, the
/// state file keeps the verdicts — and they are what a test measures the
/// responsiveness against.
#[derive(Debug, Default)]
pub struct Stats {
    /// How many connections have been accepted.
    pub accepted: AtomicU64,
    /// How many connections are open right now.
    pub live: AtomicI64,
    /// How many connections have finished, one way or another.
    pub closed: AtomicU64,
    /// How many rounds have been queued for verification.
    pub queued: AtomicU64,
    /// How many rounds verified clean.
    pub verified: AtomicU64,
    /// How many rounds did not verify.
    pub failed: AtomicU64,
}

/// The state the accept loop, the connections and the workers share.
struct Shared {
    /// The config the service runs with.
    cfg: ArchiveConfig,
    /// The bounded queue of rounds waiting to be verified.
    queue: SyncSender<Job>,
    /// Set when the service is asked to stop.
    stop: AtomicBool,
    /// The counters.
    stats: Stats,
    /// How a round is verified.
    verifier: RwLock<Verifier>,
}

/// The archive service.
///
/// The service is built by [`Server::bind`], which opens the listener, and
/// then runs by [`Server::serve`], which never returns until the service is
/// asked to stop.
pub struct Server {
    /// The listener the accept loop takes its connections from.
    listener: TcpListener,
    /// The state everything shares.
    shared: Arc<Shared>,
    /// The verification workers.
    workers: Vec<JoinHandle<()>>,
}

impl Server {
    /// Open the listener and start the verification workers.
    ///
    /// The listener is non-blocking and polled: the service has to be able to
    /// stop, and a blocked `accept` cannot be woken up without a signal
    /// handler, which the archive does not have.
    pub fn bind(cfg: ArchiveConfig) -> io::Result<Server> {
        let listener = TcpListener::bind(&cfg.listen)?;
        listener.set_nonblocking(true)?;

        // The queue is bounded: a burst of pushes is held, not swallowed.
        let capacity = cfg.queue_capacity.max(1);
        let (queue, receiver) = sync_channel::<Job>(capacity);
        let receiver = Arc::new(Mutex::new(receiver));

        let shared = Arc::new(Shared {
            cfg,
            queue,
            stop: AtomicBool::new(false),
            stats: Stats::default(),
            verifier: RwLock::new(Arc::new(|log: &RoundLog| verify::verify_log(log))),
        });

        let mut workers = Vec::new();
        for _ in 0..shared.cfg.workers.max(1) {
            let shared = Arc::clone(&shared);
            let receiver = Arc::clone(&receiver);
            workers.push(thread::spawn(move || worker(shared, receiver)));
        }

        Ok(Server {
            listener,
            shared,
            workers,
        })
    }

    /// Install the way a round is verified.
    ///
    /// The default is [`verify::verify_log`]; this replaces it. The tests
    /// use it to hold a verification open and measure the accept loop.
    pub fn with_verifier(self, verifier: Verifier) -> Server {
        *self.shared.verifier.write().expect("the verifier's lock") = verifier;
        self
    }

    /// The address the service listens on.
    ///
    /// A config that says port 0 gets a free port, and this is how the
    /// caller learns which one it got.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// The counters the service keeps.
    pub fn stats(&self) -> &Stats {
        &self.shared.stats
    }

    /// The config the service runs with.
    pub fn config(&self) -> &ArchiveConfig {
        &self.shared.cfg
    }

    /// Take the connections, until the service is asked to stop.
    ///
    /// Every connection is handed to a thread of its own, so a push that
    /// takes a long time to verify never holds the loop up. The loop itself
    /// does nothing but accept and greet.
    pub fn serve(&self) -> io::Result<()> {
        loop {
            if self.shared.stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            match self.listener.accept() {
                Ok((stream, peer)) => self.spawn(stream, peer),
                // A quiet listener: wait a moment and look again.
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL);
                }
                // A failed accept is not a failed service: the loop carries
                // on, because the next connection still has to be served.
                Err(err) => {
                    eprintln!("lm-archive: accept failed: {err}");
                    thread::sleep(ACCEPT_POLL);
                }
            }
        }
    }

    /// Ask the service to stop.
    ///
    /// The accept loop notices within a poll, and the workers stop when
    /// their queue is empty.
    pub fn shutdown(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }

    /// Wait for the verification workers to finish.
    ///
    /// The service has to be shut down first: the workers stop when they are
    /// asked to, not when the queue happens to be empty.
    pub fn join_workers(&mut self) {
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }

    /// Take one connection and give it a thread of its own.
    fn spawn(&self, stream: TcpStream, peer: SocketAddr) {
        let shared = Arc::clone(&self.shared);
        shared.stats.accepted.fetch_add(1, Ordering::Relaxed);
        shared.stats.live.fetch_add(1, Ordering::Relaxed);
        thread::spawn(move || {
            if let Err(err) = connection(stream, &shared) {
                // A refused push is the answer the peer gets, not a failure
                // of the service: it is logged and the service carries on.
                eprintln!("lm-archive: {peer}: {err}");
            }
            shared.stats.live.fetch_sub(1, Ordering::Relaxed);
            shared.stats.closed.fetch_add(1, Ordering::Relaxed);
        });
    }
}

/// Take one connection in: greet, read the push, store it, verify it, answer.
fn connection(stream: TcpStream, shared: &Shared) -> Result<IngestReport, IngestError> {
    let cfg = &shared.cfg;

    // A stalled peer must not pin a thread forever, and a push is a burst
    // of small frames: the latency of Nagle's algorithm is not wanted.
    stream
        .set_read_timeout(Some(Duration::from_secs(cfg.read_timeout_s)))
        .map_err(|err| IngestError::Io(err.to_string()))?;
    stream
        .set_nodelay(true)
        .map_err(|err| IngestError::Io(err.to_string()))?;

    let mut writer = stream
        .try_clone()
        .map_err(|err| IngestError::Io(err.to_string()))?;
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|err| IngestError::Io(err.to_string()))?,
    );

    // The greeting: the peer learns the archive is there, and which
    // protocol it speaks, before it sends a byte.
    protocol::write_frame(
        &mut writer,
        &Msg::Hello {
            proto_version: lm_proto::PROTO_VERSION,
            node: NodeKind::Archive,
            caps: Vec::new(),
        },
    )
    .map_err(|err| IngestError::Io(err.to_string()))?;
    writer
        .flush()
        .map_err(|err| IngestError::Io(err.to_string()))?;

    let log = match protocol::read_push(&mut reader, cfg) {
        Ok(log) => log,
        Err(err) => return Err(protocol::refuse(&mut writer, err)),
    };

    let store = cfg.store();
    let name = store.free_round_name(log.header.seed, crate::store::now_unix_secs());
    let path = protocol::store_round(&store, &log, &name)?;

    let verified = if cfg.verify_on_arrival {
        // The replay happens on the worker: a slow round must not hold the
        // accept loop up. The connection waits for the verdict — it is the
        // thread of this connection, not the loop's.
        let (reply, verdict) = channel::<VerifyReport>();
        shared
            .queue
            .send(Job {
                log: log.clone(),
                name: name.clone(),
                reply,
            })
            .map_err(|_| IngestError::Store("the verification worker is gone".to_owned()))?;
        shared.stats.queued.fetch_add(1, Ordering::Relaxed);
        Some(
            verdict
                .recv()
                .map_err(|_| IngestError::Store("the verification worker is gone".to_owned()))?,
        )
    } else {
        None
    };

    // The store keeps as many rounds as the config says; 0 keeps them all.
    if cfg.retain_rounds > 0 {
        let _ = store.prune(cfg.retain_rounds);
    }

    protocol::write_verdict(&mut writer, verified.as_ref(), &log, &name)
        .map_err(|err| IngestError::Io(err.to_string()))?;

    let decisions = log.segments.iter().map(|s| s.decisions.len()).sum();
    let hashes = log.segments.iter().map(|s| s.hashes.len()).sum();
    Ok(IngestReport {
        name,
        path,
        decisions,
        hashes,
        verified,
    })
}

/// The verification worker: take a round from the queue and replay it.
fn worker(shared: Arc<Shared>, receiver: Arc<Mutex<Receiver<Job>>>) {
    let store = shared.cfg.store();
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        // The lock is held only for the wait, never for the replay: two
        // workers must be able to verify two rounds at the same time.
        let job = {
            let guard = receiver.lock().expect("the queue's lock");
            match guard.recv_timeout(WORKER_POLL) {
                Ok(job) => job,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        };

        let verifier = {
            let guard = shared.verifier.read().expect("the verifier's lock");
            Arc::clone(&guard)
        };
        let report = verifier(&job.log);

        // The report is written the way every other file of the store is
        // written: atomically, so a reader never sees half of one.
        let _ = store.save_report(&job.name, &report);
        let _ = protocol::record_verdict(&store, &job.name, &report);

        if report.ok {
            shared.stats.verified.fetch_add(1, Ordering::Relaxed);
        } else {
            shared.stats.failed.fetch_add(1, Ordering::Relaxed);
        }

        // The verdict goes back to the connection that is waiting for it.
        let _ = job.reply.send(report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{round, Scratch};
    use lm_core::log::RoundLog;
    use std::io::Read;
    use std::process::Command;
    use std::sync::atomic::AtomicBool;

    /// A config that keeps everything in a scratch directory and listens on a
    /// free port of the loopback.
    fn config(scratch: &Scratch) -> ArchiveConfig {
        ArchiveConfig {
            listen: "127.0.0.1:0".to_owned(),
            store_dir: scratch.path().to_path_buf(),
            ..ArchiveConfig::default()
        }
    }

    /// The service, running in a thread of its own.
    struct Running {
        server: Arc<Server>,
        addr: SocketAddr,
    }

    impl Running {
        /// Bind the service and start serving it.
        fn start(cfg: ArchiveConfig) -> Running {
            Running::start_with(cfg, Arc::new(|log: &RoundLog| verify::verify_log(log)))
        }

        /// Bind the service and start serving it, with its own verifier.
        fn start_with(cfg: ArchiveConfig, verifier: Verifier) -> Running {
            let server = Arc::new(
                Server::bind(cfg)
                    .expect("the service binds")
                    .with_verifier(verifier),
            );
            let addr = server.local_addr().expect("the address");
            let serving = Arc::clone(&server);
            thread::spawn(move || {
                let _ = serving.serve();
            });
            Running { server, addr }
        }

        /// Stop the service.
        fn stop(&self) {
            self.server.shutdown();
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Read one frame from a peer.
    fn read_frame(stream: &mut TcpStream, max: u64) -> Option<Msg> {
        let mut reader = BufReader::new(stream.try_clone().expect("the stream clones"));
        protocol::read_frame(&mut reader, max)
            .ok()
            .map(|frame| frame.msg)
    }

    /// Push a round: the header, the decisions, the hashes, the goodbye.
    fn push(stream: &mut TcpStream, log: &RoundLog) {
        protocol::write_frame(stream, &Msg::Header(log.header.clone()))
            .expect("the header goes out");
        for segment in &log.segments {
            for stamped in &segment.decisions {
                protocol::write_frame(stream, &Msg::Stamped(stamped.clone()))
                    .expect("the decision goes out");
            }
        }
        for (tick, hash) in log.hashes() {
            protocol::write_frame(stream, &Msg::StateHash { tick, hash })
                .expect("the hash goes out");
        }
        for refused in log.refusals() {
            protocol::write_frame(stream, &Msg::Refused(refused)).expect("the refusal goes out");
        }
        protocol::write_frame(
            stream,
            &Msg::Bye {
                reason: "round over".to_owned(),
            },
        )
        .expect("the goodbye goes out");
        stream.flush().expect("the frames are flushed");
    }

    /// Wait for a condition, up to a deadline.
    fn wait_for(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if condition() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        condition()
    }

    #[test]
    fn accept_loop_stays_responsive() {
        let scratch = Scratch::new("accept-responsive");
        let cfg = config(&scratch);

        // A verification that is deliberately slow: it waits for the test to
        // let it finish.
        let started = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let verifier: Verifier = {
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            Arc::new(move |log: &RoundLog| {
                started.store(true, Ordering::SeqCst);
                while !release.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(5));
                }
                verify::verify_log(log)
            })
        };
        let running = Running::start_with(cfg.clone(), verifier);
        let log = round(1, 300);

        // The first client pushes a round: the worker takes it and holds it.
        let mut first = TcpStream::connect(running.addr).expect("the first client connects");
        let greeting = read_frame(&mut first, cfg.max_log_bytes).expect("the greeting");
        assert!(
            matches!(greeting, Msg::Hello { .. }),
            "the service greets the peer: {greeting:?}"
        );
        push(&mut first, &log);
        assert!(
            wait_for(|| started.load(Ordering::SeqCst)),
            "the worker picked the round up"
        );

        // The verification is in flight. A new connection must still be
        // accepted, and greeted, promptly.
        let started_at = std::time::Instant::now();
        let mut second = TcpStream::connect(running.addr).expect("the second client connects");
        let greeting = read_frame(&mut second, cfg.max_log_bytes).expect("the second greeting");
        let waited = started_at.elapsed();
        assert!(
            matches!(greeting, Msg::Hello { .. }),
            "the service greets the peer: {greeting:?}"
        );
        assert!(
            waited < Duration::from_secs(2),
            "the greeting took {waited:?} while a verification was in flight"
        );

        // The first push still gets its verdict.
        release.store(true, Ordering::SeqCst);
        let verdict = read_frame(&mut first, cfg.max_log_bytes).expect("the verdict comes back");
        assert!(
            matches!(verdict, Msg::StateHash { .. }),
            "the round verified clean: {verdict:?}"
        );
    }

    #[test]
    fn verification_is_yielded() {
        // The service's own priority: a process started the way the service
        // starts itself is at the configured priority, and the kernel is the
        // one that says so.
        let scratch = Scratch::new("yielded");
        let cfg = ArchiveConfig {
            nice: 10,
            ionice_class: 3,
            ..config(&scratch)
        };

        let exe = std::env::current_exe().expect("the test binary");
        let mut cmd = Command::new(exe);
        cmd.arg("--exact")
            .arg("server::tests::the_yielded_child")
            .arg("--nocapture");
        crate::priority::wrap(&mut cmd, &cfg).expect("the command is wrapped");
        let mut child = cmd.spawn().expect("the child starts");

        let pid = child.id();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut nice = 0;
        while std::time::Instant::now() < deadline {
            nice = read_nice(pid);
            if nice >= cfg.nice {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            nice >= cfg.nice,
            "the service's niceness is {nice}, not at least {}",
            cfg.nice
        );

        // The I/O class is idle.
        let class = read_io_class(pid);
        assert_eq!(
            class,
            crate::priority::IO_CLASS_IDLE,
            "the service's I/O class is {class}"
        );

        // And the service says the same about itself, from the inside.
        let status = child.wait().expect("the child finishes");
        let verdict = std::env::temp_dir().join(format!("lm-archive-yielded-{pid}.txt"));
        let text = std::fs::read_to_string(&verdict).expect("the child's verdict is there");
        let _ = std::fs::remove_file(&verdict);
        assert!(
            status.success(),
            "the service's own verdict was a failure:\n{text}"
        );
        assert!(
            text.contains(&format!("io class: {}", crate::priority::IO_CLASS_IDLE)),
            "the service does not see itself at the idle I/O class:\n{text}"
        );
    }

    /// The test the `verification_is_yielded` test runs in its child.
    #[test]
    fn the_yielded_child() {
        // Only the child that was started under the wrap does the work: the
        // parent's own run of this test is a no-op.
        if !crate::priority::already_applied() {
            return;
        }
        // The niceness the kernel gave it, and the I/O class it reads back.
        let nice = crate::priority::current_nice().expect("the niceness reads");
        let class = crate::priority::current_io_class().expect("the class reads");
        println!("child nice: {nice}");
        println!("child io class: {class}");
        // The verdict, for the parent to read: the process's own view of
        // its own priority.
        let path =
            std::env::temp_dir().join(format!("lm-archive-yielded-{}.txt", std::process::id()));
        std::fs::write(&path, format!("nice: {nice}\nio class: {class}\n"))
            .expect("the verdict is written");
        assert!(
            crate::priority::is_low_priority(),
            "the service does not think it is at a low priority"
        );
        assert_eq!(
            class,
            crate::priority::IO_CLASS_IDLE,
            "the service is not at the idle I/O class"
        );
        // Stay alive long enough for the parent to look at this process from
        // the outside.
        thread::sleep(Duration::from_secs(2));
    }

    /// The niceness of a process, read the way the kernel writes it.
    fn read_nice(pid: u32) -> i32 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("the stat file");
        let at = stat.rfind(')').expect("the command name");
        stat[at + 1..]
            .split_whitespace()
            .nth(16)
            .and_then(|value| value.parse::<i32>().ok())
            .expect("the niceness")
    }

    /// The I/O class of a process, read back with `ionice`.
    fn read_io_class(pid: u32) -> u8 {
        let output = Command::new("ionice")
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .expect("ionice runs");
        let text = String::from_utf8_lossy(&output.stdout);
        let text = text.trim();
        if text.starts_with("idle") {
            return crate::priority::IO_CLASS_IDLE;
        }
        if text.starts_with("none") {
            return 0;
        }
        if text.starts_with("best-effort") {
            return 2;
        }
        panic!("ionice said {text:?}");
    }

    #[test]
    fn slow_peer_does_not_pin_a_thread() {
        let scratch = Scratch::new("slow-peer");
        let mut cfg = config(&scratch);
        cfg.read_timeout_s = 1;
        let running = Running::start(cfg.clone());
        let log = round(1, 300);

        // A peer that connects and says nothing at all.
        let mut client = TcpStream::connect(running.addr).expect("the client connects");
        let _ = read_frame(&mut client, cfg.max_log_bytes).expect("the greeting");

        // The service drops it once the read timeout is up, and says why.
        let mut buf = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut closed = false;
        while std::time::Instant::now() < deadline {
            client
                .set_read_timeout(Some(Duration::from_millis(500)))
                .expect("the timeout is set");
            let mut chunk = [0u8; 256];
            match client.read(&mut chunk) {
                Ok(0) => {
                    closed = true;
                    break;
                }
                Ok(got) => buf.extend_from_slice(&chunk[..got]),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) if err.kind() == io::ErrorKind::TimedOut => continue,
                Err(err) => panic!("the socket failed: {err}"),
            }
        }
        assert!(closed, "the silent peer was never dropped");

        // And the thread it held is gone: the service is not one short.
        assert!(
            wait_for(|| running.server.stats().live.load(Ordering::Relaxed) == 0),
            "the connection's thread is still running"
        );

        // The service is still serving.
        let mut client = TcpStream::connect(running.addr).expect("the client connects again");
        let greeting = read_frame(&mut client, cfg.max_log_bytes).expect("the greeting");
        assert!(matches!(greeting, Msg::Hello { .. }));
        let _ = log;
    }

    #[test]
    fn oversized_push_is_refused_early() {
        let scratch = Scratch::new("oversized");
        let mut cfg = config(&scratch);
        cfg.max_log_bytes = 1024;
        let running = Running::start(cfg.clone());

        let mut client = TcpStream::connect(running.addr).expect("the client connects");
        let _ = read_frame(&mut client, cfg.max_log_bytes).expect("the greeting");

        // A frame that claims to be a hundred megabytes. The body never
        // arrives: the service must refuse on the length alone, before it
        // waits for a body that is not coming.
        let claimed = 100 * 1024 * 1024u32;
        client
            .write_all(&claimed.to_le_bytes())
            .expect("the length goes out");
        client.flush().expect("the length is flushed");

        let verdict = read_frame(&mut client, cfg.max_log_bytes).expect("the refusal comes back");
        match verdict {
            Msg::Bye { reason } => assert!(
                reason.contains("over the") && reason.contains("cap"),
                "the refusal names the cap: {reason}"
            ),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn service_survives_a_bad_connection() {
        let scratch = Scratch::new("bad-connection");
        let cfg = config(&scratch);
        let running = Running::start(cfg.clone());
        let log = round(1, 300);

        // A client that disconnects in the middle of a frame.
        {
            let mut client = TcpStream::connect(running.addr).expect("the client connects");
            let _ = read_frame(&mut client, cfg.max_log_bytes).expect("the greeting");
            client
                .write_all(&[0x10, 0x00])
                .expect("half a length prefix goes out");
            client.flush().expect("the bytes are flushed");
            let _ = client.shutdown(std::net::Shutdown::Both);
        }

        // The service is still there, and it still works.
        assert!(
            wait_for(|| running.server.stats().closed.load(Ordering::Relaxed) >= 1),
            "the broken connection was dealt with"
        );
        let mut client = TcpStream::connect(running.addr).expect("the client connects again");
        let greeting = read_frame(&mut client, cfg.max_log_bytes).expect("the greeting");
        assert!(matches!(greeting, Msg::Hello { .. }));

        // And a good push still verifies.
        push(&mut client, &log);
        let verdict = read_frame(&mut client, cfg.max_log_bytes).expect("the verdict comes back");
        assert!(
            matches!(verdict, Msg::StateHash { .. }),
            "the round verified clean: {verdict:?}"
        );
    }

    #[test]
    fn the_queue_is_bounded() {
        // The queue holds the rounds that are waiting for the worker, and it
        // holds no more than the config says.
        let scratch = Scratch::new("bounded-queue");
        let mut cfg = config(&scratch);
        cfg.queue_capacity = 2;
        let running = Running::start(cfg.clone());
        assert_eq!(running.server.config().queue_capacity, 2);
    }
}
