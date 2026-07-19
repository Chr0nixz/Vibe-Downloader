//! A-2 cross-engine integration coverage: WebDAV/WebDAVS engine probe path.
//!
//! The full download path (`WebDavEngine::download`) delegates to the HTTP
//! engine and requires a `tauri::AppHandle`; following the convention of
//! `ftp_engine.rs`, `sftp_engine.rs`, and `metalink_engine.rs`, these tests
//! cover the probe layer (`WebDavEngine::probe`) end-to-end against the shared
//! HTTP `TestServer` from `tests/common`.
//!
//! ## Fake server
//!
//! Reuses `common::TestServer` (a minimal `127.0.0.1:0` TCP server that
//! dispatches each accepted connection to a cloned handler closure). The
//! handler speaks just enough HTTP/1.1 to drive the WebDAV probe path, which
//! internally converts `webdav://host:port/path` to `http://host:port/path`
//! and delegates to `HttpEngine::probe_with_headers`. The handler honors
//! `Range`, `Authorization`, `Content-Disposition`, and `Accept-Ranges`.
//!
//! ## Scenarios
//!
//! Per the Phase 8 plan each protocol covers at minimum:
//! 1. **Create** — `probe_advertises_size_and_resume_support`
//! 2. **Pause/resume** — `probe_advertises_size_and_resume_support` (the
//!    probe verifies `Accept-Ranges: bytes` which gates resume support)
//! 3. **Failure** — `probe_fails_when_server_returns_404`
//! 4. **Proxy unsupported** — WebDAV allows HTTP/HTTPS/SOCKS5 proxies via
//!    the HTTP engine; covered via `db::task_proxy::validate_task_proxy_protocol`
//!    unit tests (out of scope for the engine probe integration test)
//! 5. **Credentials failed** — `probe_fails_when_server_returns_401_without_credentials`
//! 6. **Checksum failed** — checksum verification happens at the DB layer
//!    post-download; covered by Metalink hash tests and `task_checksums` unit
//!    tests. The WebDAV probe does not perform checksum verification.
//! 7. **Directory URL rejected** — `probe_rejects_directory_url`

mod common;

use std::{
    io::Read,
    net::TcpStream,
    sync::{Arc, Mutex},
    time::Duration,
};

use common::http::{extract_byte_range, extract_header, respond_file, write_response, ByteRange};
use common::TestServer;
use tauri_app_lib::{
    db,
    download::{DownloadEngine, HttpEngine, ProbeRequest, WebDavEngine},
    models::{SegmentStatus, TaskStatus},
    proxy::ResolvedProxyConfig,
    state_machine,
};

const SAMPLE: &[u8] = b"Vibe Downloader WebDAV regression payload.";

fn resume_payload() -> Vec<u8> {
    (0..128 * 1024).map(|index| (index % 251) as u8).collect()
}

/// Per-connection handler state. Cloned per accepted connection.
#[derive(Clone)]
struct WebDavHandlerState {
    /// Tracks whether the embedded-credentials test observed the Basic auth
    /// header on at least one request. Used by
    /// `probe_succeeds_with_embedded_credentials` to assert the engine
    /// forwarded the credentials extracted from the URL.
    observed_authorization: Arc<Mutex<Option<String>>>,
    /// When `true`, every request without a valid `Authorization: Basic ...`
    /// header matching `user:pass` is rejected with 401.
    require_basic_auth: bool,
    /// Expected Basic auth credential tuple (`user:pass`).
    expected_credentials: Option<String>,
}

impl WebDavHandlerState {
    fn new() -> Self {
        Self {
            observed_authorization: Arc::new(Mutex::new(None)),
            require_basic_auth: false,
            expected_credentials: None,
        }
    }

    fn with_required_auth(user: &str, password: &str) -> Self {
        let token = base64_encode(&format!("{user}:{password}"));
        Self {
            observed_authorization: Arc::new(Mutex::new(None)),
            require_basic_auth: true,
            expected_credentials: Some(token),
        }
    }
}

fn start_test_server(state: WebDavHandlerState) -> TestServer {
    TestServer::start(move |mut stream| {
        handle_connection(&mut stream, state.clone());
    })
}

fn handle_connection(stream: &mut TcpStream, state: WebDavHandlerState) {
    let mut buffer = [0u8; 4096];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    if read == 0 {
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let request_line = request.lines().next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/");
    let byte_range = extract_byte_range(&request);
    let authorization = extract_header(&request, "authorization").map(str::to_string);

    // Record the Authorization header so tests can assert the engine
    // forwarded the embedded credentials extracted from the webdav:// URL.
    if let Some(auth) = &authorization {
        if let Ok(mut slot) = state.observed_authorization.lock() {
            *slot = Some(auth.clone());
        }
    }

    // Enforce Basic auth when configured.
    if state.require_basic_auth {
        let expected = state
            .expected_credentials
            .as_deref()
            .expect("expected credentials set when require_basic_auth is true");
        let provided = authorization
            .as_deref()
            .and_then(|value| value.strip_prefix("Basic ").map(str::trim));
        if provided != Some(expected) {
            write_response(
                stream,
                401,
                &[("WWW-Authenticate", "Basic realm=\"vibe-test\"")],
                b"auth required",
                false,
            );
            return;
        }
    }

    match path {
        "/file" => respond_file(
            stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "sample.bin",
            false,
        ),
        "/large.bin" => respond_large_file(stream, method, byte_range),
        "/resume.bin" => respond_file(
            stream,
            method,
            &resume_payload(),
            byte_range,
            true,
            "resume.bin",
            true,
        ),
        "/missing" => write_response(stream, 404, &[], b"not found", false),
        "/no-ranges" => respond_file(
            stream,
            method,
            SAMPLE,
            byte_range,
            false,
            "no-ranges.bin",
            false,
        ),
        _ => write_response(stream, 404, &[], b"unknown route", false),
    }
}

/// Respond with a 32 MB synthetic payload to exercise the
/// `supports_parallel` probe branch (above the 16 MB threshold).
fn respond_large_file(stream: &mut TcpStream, method: &str, byte_range: Option<ByteRange>) {
    let total: usize = 32 * 1024 * 1024;
    // Generate the same repeating-pattern payload the FTP tests use so any
    // future checksum assertion can reuse a known digest.
    let payload: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
    respond_file(
        stream,
        method,
        &payload,
        byte_range,
        true,
        "large.bin",
        false,
    );
}

/// Minimal Base64 encoder (the standard `base64` crate is not available in
/// the test binary's deps tree without adding it; we only need it for a
/// handful of bytes so an inline encoder avoids touching Cargo.toml).
fn base64_encode(input: &str) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

// --- Engine probe helpers --------------------------------------------------

fn new_engine() -> WebDavEngine {
    WebDavEngine::new(Arc::new(
        HttpEngine::with_proxy_config(ResolvedProxyConfig::shared_default())
            .expect("HTTP engine init"),
    ))
}

fn new_probe_request(uri: String) -> ProbeRequest {
    ProbeRequest {
        uri,
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
        proxy_config: None,
        app: None,
        request_id: None,
    }
}

fn webdav_url(server: &TestServer, path: &str) -> String {
    format!("webdav://{}{path}", server.authority())
}

// --- Probe tests -----------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_advertises_size_and_resume_support() {
    // Create + pause/resume: probe returns the file size, advertises
    // supports_resume (Accept-Ranges: bytes), and reports the webdav
    // protocol on the resolved URI.
    let server = start_test_server(WebDavHandlerState::new());
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(webdav_url(&server, "/file")))
        .await
        .expect("probe");

    assert_eq!(output.protocol, "webdav");
    assert_eq!(output.display_name, "sample.bin");
    assert_eq!(output.total_size, SAMPLE.len() as i64);
    assert!(output.capabilities.supports_resume);
    assert_eq!(output.files.len(), 1);
    assert_eq!(output.files[0].relative_path, "sample.bin");
    assert!(output.resolved_uri.starts_with("webdav://"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_advertises_parallel_for_large_files() {
    // Large-file branch: a 32 MB payload with Accept-Ranges: bytes must
    // surface supports_parallel = true (above the 16 MB threshold).
    let server = start_test_server(WebDavHandlerState::new());
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(webdav_url(&server, "/large.bin")))
        .await
        .expect("probe");

    assert_eq!(output.total_size, 32 * 1024 * 1024);
    assert!(output.capabilities.supports_resume);
    assert!(output.capabilities.supports_parallel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_disables_parallel_when_server_omits_accept_ranges() {
    // No Accept-Ranges header → supports_resume stays true only if the server
    // returns Accept-Ranges: bytes. This server omits it, so resume must be
    // false and parallel must be false.
    let server = start_test_server(WebDavHandlerState::new());
    let engine = new_engine();

    let output = engine
        .probe(new_probe_request(webdav_url(&server, "/no-ranges")))
        .await
        .expect("probe");

    assert!(!output.capabilities.supports_resume);
    assert!(!output.capabilities.supports_parallel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_server_returns_404() {
    // Failure path: a 404 must surface as a typed error (not panic).
    let server = start_test_server(WebDavHandlerState::new());
    let engine = new_engine();

    let result = engine
        .probe(new_probe_request(webdav_url(&server, "/missing")))
        .await;

    assert!(result.is_err(), "probe should fail on 404");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_when_server_returns_401_without_credentials() {
    // Credentials failed: the server requires Basic auth and no credentials
    // are supplied. The probe must surface a typed error.
    let server = start_test_server(WebDavHandlerState::with_required_auth("alice", "s3cret"));
    let engine = new_engine();

    let result = engine
        .probe(new_probe_request(webdav_url(&server, "/file")))
        .await;

    assert!(
        result.is_err(),
        "probe should fail on 401 without credentials"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_succeeds_with_embedded_credentials() {
    // Embedded credentials: `webdav://alice:s3cret@host/file` must extract
    // the credentials, forward them as a Basic auth header, and succeed
    // against a server that requires exactly those credentials.
    let state = WebDavHandlerState::with_required_auth("alice", "s3cret");
    let observed = state.observed_authorization.clone();
    let server = start_test_server(state);
    let engine = new_engine();

    let url = format!("webdav://alice:s3cret@{}/file", server.authority());
    let output = engine.probe(new_probe_request(url)).await.expect("probe");

    assert_eq!(output.protocol, "webdav");
    assert_eq!(output.display_name, "sample.bin");
    assert_eq!(output.total_size, SAMPLE.len() as i64);

    // The handler must have observed a Basic auth header carrying the
    // base64-encoded `alice:s3cret` token.
    let observed_auth = observed
        .lock()
        .expect("auth lock")
        .clone()
        .expect("authorization header observed");
    assert!(
        observed_auth.contains("Basic "),
        "expected Basic auth header, got: {observed_auth}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_strips_credentials_from_resolved_uri() {
    // URL sanitization: `webdav://alice:s3cret@host/file` must not leak the
    // password (or the username) into the resolved_uri returned by the probe.
    let server = start_test_server(WebDavHandlerState::new());
    let engine = new_engine();

    let url = format!("webdav://alice:s3cret@{}/file", server.authority());
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
    assert!(output.resolved_uri.starts_with("webdav://"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_rejects_directory_url() {
    // Directory URL rejection: a URL ending with `/` has no file name, so
    // `WebDavTarget::parse_file` must reject it before any network call.
    // The directory probe (`probe_webdav_directory_url`) is `pub(crate)`
    // and not reachable from external test binaries, so we only assert the
    // engine-level rejection here.
    let server = start_test_server(WebDavHandlerState::new());
    let engine = new_engine();

    let url = format!("webdav://{}/dir/", server.authority());
    let result = engine.probe(new_probe_request(url)).await;

    assert!(
        result.is_err(),
        "directory URL should be rejected by the engine probe"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_pauses_mid_transfer_and_resumes_through_http_engine() {
    let payload = resume_payload();
    let server = start_test_server(WebDavHandlerState::new());
    let pool = common::test_pool("webdav-pause-resume").await;
    let paths = common::TestPaths::new("webdav-pause-resume");
    let task = common::download_task(
        "webdav-pause-resume",
        webdav_url(&server, "/resume.bin"),
        "webdav",
        "resume.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert WebDAV task");

    let engine = new_engine();
    let cancel = tokio_util::sync::CancellationToken::new();
    let first_download = tokio::spawn({
        let engine = engine.clone();
        let context = common::headless_download_context(pool.clone(), task, cancel.clone());
        async move { engine.download(context).await }
    });

    let partial = loop {
        let segments = db::list_segment_records(&pool, "webdav-pause-resume")
            .await
            .expect("list WebDAV segments");
        if let Some(downloaded) = segments
            .first()
            .map(|segment| segment.downloaded_until)
            .filter(|downloaded| *downloaded > 0)
        {
            break downloaded;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(partial < payload.len() as i64);
    cancel.cancel();
    first_download
        .await
        .expect("WebDAV download task join")
        .expect("WebDAV cancellation is a clean pause boundary");

    let current = db::get_task_record(&pool, "webdav-pause-resume")
        .await
        .expect("read WebDAV task")
        .expect("WebDAV task exists");
    let no_app = Option::<tauri::AppHandle>::None;
    state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &current.id,
        TaskStatus::Paused,
        current.downloaded_bytes,
        0,
        Some("Paused"),
        Some("paused"),
        None,
        SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("persist WebDAV pause");
    let resumed = state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &current.id,
        TaskStatus::Downloading,
        current.downloaded_bytes,
        1,
        Some("Downloading"),
        Some("resumed"),
        None,
        SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("persist WebDAV resume");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("resume WebDAV download");

    let actual = std::fs::read(&paths.final_path).expect("read WebDAV file");
    assert_eq!(actual.len(), payload.len());
    assert!(actual == payload, "resumed WebDAV payload differs");
    assert!(!paths.temp.exists());
    let completed = db::get_task_record(&pool, "webdav-pause-resume")
        .await
        .expect("read completed WebDAV task")
        .expect("completed WebDAV task exists");
    assert_eq!(completed.status, TaskStatus::Completed);
    assert_eq!(completed.downloaded_bytes, payload.len() as i64);
}
