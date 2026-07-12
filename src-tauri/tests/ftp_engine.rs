//! A-2 cross-engine integration coverage: FTP/FTPS engine probe path.
//!
//! The full download path (`run_ftp_download`) requires a `tauri::AppHandle`
//! and is exercised only via the engine runtime; following the convention of
//! `sftp_engine.rs` and `metalink_engine.rs`, these tests cover the probe
//! layer (`FtpEngine::probe`) end-to-end against a minimal in-process FTP
//! fake server, plus the URL-parsing and target-sanitization helpers that
//! the probe path depends on.
//!
//! ## Fake server
//!
//! `FtpTestServer` speaks just enough of RFC 959 to drive the probe:
//! `USER`/`PASS` (always accept by default, optional reject),
//! `TYPE I`, `PASV` (returns a new data-port for the client to connect to),
//! `SIZE`, `MDTM`, `REST <offset>`, `RETR <path>`, `QUIT`. Files are served
//! from an in-memory `HashMap`. The server binds `127.0.0.1:0` for both the
//! control and data channels so tests run in parallel without port clashes.
//!
//! ## Scenarios
//!
//! Per the Phase 8 plan each protocol covers at minimum:
//! 1. **Create** — `probe_advertises_size_and_parallel_support`
//! 2. **Pause/resume** — `probe_reports_supports_resume_when_rest_zero_succeeds`
//! 3. **Failure** — `probe_fails_when_server_returns_530_for_user_pass`
//! 4. **Proxy unsupported** — FTP allows SOCKS5 only; covered via
//!    `db::task_proxy::validate_task_proxy_protocol` unit tests (out of
//!    scope for the engine probe integration test, see `tests/proxy.rs`)
//! 5. **Credentials failed** — `probe_fails_when_server_returns_530_for_user_pass`
//! 6. **Checksum failed** — checksum verification happens at the DB layer
//!    post-download; covered by Metalink hash tests and `task_checksums` unit
//!    tests. The FTP probe does not perform checksum verification.
//! 7. **Directory probe** — covered by `commands/tasks.rs::probe_ftp_directory_url`
//!    command tests; the `probe_ftp_directory_url` function is `pub(crate)`
//!    and not re-exported from `download/mod.rs`, so it is not reachable from
//!    external test binaries.

mod common;

use std::{
    collections::HashMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use tauri_app_lib::{
    download::{DownloadEngine, FtpEngine, ProbeRequest},
    proxy::ResolvedProxyConfig,
};

// --- In-process FTP fake server --------------------------------------------

/// Configuration for the FTP fake server.
#[derive(Clone, Default)]
struct FtpServerConfig {
    /// Files to serve, keyed by absolute path (e.g. `/file.bin`).
    files: HashMap<String, Vec<u8>>,
    /// When `true`, the server rejects every `USER`/`PASS` with 530.
    reject_auth: bool,
    /// When `true`, the server refuses `REST <offset>` with 501. Used to
    /// exercise the `supports_resume = false` probe branch.
    reject_rest: bool,
    /// When `true`, the server omits `SIZE` (returns 550). Used to exercise
    /// the unknown-size probe branch where `total_size` falls back to 0.
    reject_size: bool,
}

/// A running FTP fake server. Drop stops the accept loop.
struct FtpTestServer {
    addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
}

impl FtpTestServer {
    fn start(config: FtpServerConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind control");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        thread::spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let cfg = config.clone();
                        thread::spawn(move || handle_ftp_session(stream, cfg));
                    }
                    Err(_) => break,
                }
            }
        });
        Self { addr, stop }
    }

    fn url(&self, path: &str) -> String {
        format!("ftp://{}/{path}", self.addr)
    }
}

impl Drop for FtpTestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept so the loop can observe `stop`.
        let _ = TcpStream::connect(self.addr);
    }
}

fn handle_ftp_session(mut stream: TcpStream, config: FtpServerConfig) {
    let _ = writeln!(stream, "220 vibe-test FTP server ready");
    let mut buf = [0u8; 512];
    // Per-session state. The engine issues commands in a fixed order
    // (USER, PASS, TYPE I, PASV, SIZE, MDTM, REST 0, QUIT), so a single
    // data-listener slot + a single REST offset are sufficient.
    let mut data_listener: Option<TcpListener> = None;
    let mut rest_offset: u64 = 0;

    loop {
        let Ok(read) = stream.read(&mut buf) else {
            return;
        };
        if read == 0 {
            return;
        }
        let line = String::from_utf8_lossy(&buf[..read]);
        // FTP commands are case-insensitive; normalize to upper for dispatch.
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let upper = trimmed.to_ascii_uppercase();

        if upper.starts_with("USER ") {
            if config.reject_auth {
                let _ = writeln!(stream, "530 Login incorrect");
            } else {
                let _ = writeln!(stream, "331 Please specify the password");
            }
        } else if upper.starts_with("PASS ") {
            if config.reject_auth {
                let _ = writeln!(stream, "530 Login incorrect");
            } else {
                let _ = writeln!(stream, "230 Login successful");
            }
        } else if upper.starts_with("TYPE ") {
            let _ = writeln!(stream, "200 Type set");
        } else if upper.starts_with("PWD") {
            let _ = writeln!(stream, "257 \"/\" is the current directory");
        } else if upper.starts_with("PASV") {
            // Open a data listener on a random port and return the 227 reply
            // with the encoded host/port tuple. The data listener is
            // one-shot: a single RETR will read from it then close.
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind data");
            let data_addr = listener.local_addr().expect("data addr");
            data_listener = Some(listener);
            let octets = data_addr.ip().to_string();
            let parts: Vec<&str> = octets.split('.').collect();
            let p1 = data_addr.port() / 256;
            let p2 = data_addr.port() % 256;
            let _ = writeln!(
                stream,
                "227 Entering Passive Mode ({},{},{},{},{p1},{p2})",
                parts[0], parts[1], parts[2], parts[3]
            );
        } else if upper.starts_with("SIZE ") {
            if config.reject_size {
                let _ = writeln!(stream, "550 Could not get file size");
            } else {
                let path = trimmed[5..].trim().to_string();
                if let Some(payload) = config.files.get(&path) {
                    let _ = writeln!(stream, "213 {}", payload.len());
                } else {
                    let _ = writeln!(stream, "550 File not found");
                }
            }
        } else if upper.starts_with("MDTM ") {
            let _ = writeln!(stream, "213 20260101000000");
        } else if upper.starts_with("REST ") {
            if config.reject_rest {
                let _ = writeln!(stream, "501 REST not supported");
            } else {
                let offset: u64 = trimmed[5..].trim().parse().unwrap_or(0);
                rest_offset = offset;
                let _ = writeln!(stream, "350 Restarting at {offset}");
            }
        } else if upper.starts_with("RETR ") {
            let path = trimmed[5..].trim().to_string();
            let _ = writeln!(stream, "150 Opening data connection");

            // Pop the data listener set by the preceding PASV, accept one
            // connection, write the file bytes (honoring REST offset), and
            // close. The accept blocks this thread until the client
            // connects to the data port, which happens immediately after
            // RETR per RFC 959. The probe never actually calls RETR (it
            // only sends USER/PASS/TYPE/PASV/SIZE/MDTM/REST/QUIT), but the
            // command is implemented so the fake server can serve real
            // downloads if a future test exercises the full download path.
            if let Some(listener) = data_listener.take() {
                if let Ok((mut data_stream, _)) = listener.accept() {
                    if let Some(payload) = config.files.get(&path) {
                        let start = usize::try_from(rest_offset).unwrap_or(0);
                        if start < payload.len() {
                            let _ = data_stream.write_all(&payload[start..]);
                        }
                    }
                    let _ = data_stream.flush();
                }
            }
            let _ = writeln!(stream, "226 Transfer complete");
        } else if upper.starts_with("QUIT") {
            let _ = writeln!(stream, "221 Goodbye");
            return;
        } else {
            let _ = writeln!(stream, "502 Command not implemented");
        }
    }
}

// --- Engine probe helpers --------------------------------------------------

fn new_engine() -> FtpEngine {
    FtpEngine::new(ResolvedProxyConfig::shared_default())
}

fn new_probe_request(uri: String) -> ProbeRequest {
    ProbeRequest {
        uri,
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
        app: None,
        request_id: None,
    }
}

/// Build a config with one file of the given size filled with a repeating
/// pattern (used so checksums would be deterministic if ever needed).
fn config_with_file(path: &str, size: usize) -> FtpServerConfig {
    let mut files = HashMap::new();
    let payload: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    files.insert(path.to_string(), payload);
    FtpServerConfig {
        files,
        reject_auth: false,
        reject_rest: false,
        reject_size: false,
    }
}

// --- Probe tests ------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_advertises_size_and_parallel_support() {
    // Create: probe returns the file size, advertises supports_resume and
    // supports_parallel (file is above the multi-connection threshold).
    let server = FtpTestServer::start(config_with_file("/payload.bin", 32 * 1024 * 1024));
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(server.url("payload.bin")))
        .await
        .expect("probe");

    assert_eq!(output.protocol, "ftp");
    assert_eq!(output.display_name, "payload.bin");
    assert_eq!(output.total_size, 32 * 1024 * 1024);
    assert!(output.capabilities.supports_resume);
    assert!(output.capabilities.supports_parallel);
    assert_eq!(output.files.len(), 1);
    assert_eq!(output.files[0].relative_path, "payload.bin");
    assert_eq!(output.files[0].size, "33554432");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_reports_supports_resume_when_rest_zero_succeeds() {
    // Pause/resume: probe runs REST 0 to verify the server honors seek.
    // When REST succeeds, supports_resume should be true.
    let server = FtpTestServer::start(config_with_file("/file.bin", 1024));
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(server.url("file.bin")))
        .await
        .expect("probe");

    assert!(output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_disables_resume_when_server_rejects_rest() {
    // Pause/resume failure: when REST returns 501 the probe must surface
    // supports_resume = false. supports_parallel must also be false because
    // the engine requires resume support to split ranges.
    let mut config = config_with_file("/file.bin", 32 * 1024 * 1024);
    config.reject_rest = true;
    let server = FtpTestServer::start(config);
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(server.url("file.bin")))
        .await
        .expect("probe");

    assert!(!output.capabilities.supports_resume);
    assert!(!output.capabilities.supports_parallel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_disables_parallel_when_size_below_threshold() {
    // Small files must not be split even when the server supports REST.
    let server = FtpTestServer::start(config_with_file("/small.bin", 1024));
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(server.url("small.bin")))
        .await
        .expect("probe");

    assert!(output.capabilities.supports_resume);
    // Below DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES (16 MB).
    assert!(!output.capabilities.supports_parallel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_server_returns_530_for_user_pass() {
    // Credentials failure / failure path: the server rejects USER/PASS
    // with 530. The probe should surface a typed error (not panic).
    let mut config = config_with_file("/file.bin", 1024);
    config.reject_auth = true;
    let server = FtpTestServer::start(config);
    let engine = new_engine();

    let result = engine
        .probe(new_probe_request(server.url("file.bin")))
        .await;

    assert!(result.is_err(), "probe should fail on 530 auth rejection");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_reports_unknown_size_when_server_rejects_size() {
    // Unknown-size fallback: when SIZE returns 550, the probe must surface
    // total_size = 0 and disable parallel support (the engine cannot plan
    // ranges for an unknown length).
    let mut config = config_with_file("/file.bin", 4096);
    config.reject_size = true;
    let server = FtpTestServer::start(config);
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(server.url("file.bin")))
        .await
        .expect("probe");

    assert_eq!(output.total_size, 0);
    assert!(!output.capabilities.supports_parallel);
    // supports_resume requires total_size > 0 per probe_target.
    assert!(!output.capabilities.supports_resume);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_strips_embedded_credentials_from_url() {
    // URL sanitization: `ftp://alice:s3cret@host/file.bin` must extract and
    // encrypt the credentials then sanitize the URL. The probe should
    // succeed against the fake server (which accepts any password) and the
    // returned `resolved_uri` must not contain the password.
    let server = FtpTestServer::start(config_with_file("/file.bin", 1024));
    let engine = new_engine();

    let url = format!("ftp://alice:s3cret@{}/file.bin", server.addr);
    let output = engine.probe(new_probe_request(url)).await.expect("probe");

    assert!(
        !output.resolved_uri.contains("s3cret"),
        "resolved_uri must not contain embedded password, got: {}",
        output.resolved_uri
    );
    assert!(
        !output.resolved_uri.contains("alice"),
        "resolved_uri must not contain embedded username, got: {}",
        output.resolved_uri
    );
}
