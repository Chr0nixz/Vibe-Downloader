pub mod http;
pub mod sftp_server;

use std::{
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use sqlx::SqlitePool;
use tauri_app_lib::{
    db,
    download::{DownloadContext, GlobalSpeedLimiter},
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
    proxy::ResolvedProxyConfig,
};

/// A minimal TCP test server for integration tests.
///
/// Each accepted connection is handed to the provided `handler` closure in a
/// dedicated thread. The handler must be `Clone` (closures capturing `Arc<_>`
/// satisfy this) so it can be cloned per connection.
///
/// The server runs on a random port (`127.0.0.1:0`) and is stopped on drop.
#[allow(dead_code)]
pub struct TestServer {
    pub base_url: String,
    stop: Arc<AtomicBool>,
}

#[allow(dead_code)]
impl TestServer {
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(TcpStream) + Send + Sync + Clone + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let (ready_tx, ready_rx) = mpsc::channel();

        thread::spawn(move || {
            let _ = ready_tx.send(());
            while !thread_stop.load(Ordering::SeqCst) {
                // Blocking accept: the thread always waits inside accept(),
                // eliminating the polling gap that caused flaky connection
                // resets under high parallelism. Drop wakes the listener via
                // a dummy connect so the loop can check thread_stop and exit.
                match listener.accept() {
                    Ok((stream, _)) => {
                        let handler = handler.clone();
                        thread::spawn(move || handler(stream));
                    }
                    Err(_) => break,
                }
            }
        });
        ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("test server ready");

        Self {
            base_url: format!("http://{addr}"),
            stop,
        }
    }

    /// Returns the `host:port` portion of the server's URL, suitable for
    /// constructing non-HTTP URLs (e.g. `ftp://host:port/...`,
    /// `webdav://host:port/...`).
    #[allow(dead_code)]
    pub fn authority(&self) -> &str {
        self.base_url.trim_start_matches("http://")
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.base_url.trim_start_matches("http://"));
    }
}

/// Temporary file paths for download tests.
#[allow(dead_code)]
pub struct TestPaths {
    pub temp: PathBuf,
    pub final_path: PathBuf,
}

#[allow(dead_code)]
impl TestPaths {
    pub fn new(label: &str) -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("vibe-downloader-{label}-{id}"));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self {
            temp: dir.join("file.bin.vibe-downloading"),
            final_path: dir.join("file.bin"),
        }
    }
}

/// Fixed ChaCha20 key so credential encryption works without an OS keyring (CI).
///
/// Matches the unit-test helper in `db::task_credentials`. The library only
/// honors `VIBE_DOWNLOADER_TEST_SECRET_KEY` under `debug_assertions`.
#[allow(dead_code)]
pub fn install_test_secret_key() {
    std::env::set_var(
        "VIBE_DOWNLOADER_TEST_SECRET_KEY",
        "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=",
    );
}

/// Opens an isolated migrated database for a real engine integration test.
#[allow(dead_code)]
pub async fn test_pool(label: &str) -> SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-engine-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

/// Builds the single-file task shape consumed by protocol download engines.
#[allow(dead_code, clippy::too_many_arguments)]
pub fn download_task(
    id: &str,
    url: String,
    protocol: &str,
    file_name: &str,
    total_size: i64,
    paths: &TestPaths,
    supports_parallel: bool,
) -> TaskRecord {
    let now = chrono::Utc::now().to_rfc3339();
    TaskRecord {
        id: id.to_string(),
        url: url.clone(),
        final_url: Some(url),
        protocol: protocol.to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: file_name.to_string(),
        save_dir: paths
            .final_path
            .parent()
            .expect("final path parent")
            .to_string_lossy()
            .to_string(),
        temp_path: Some(paths.temp.to_string_lossy().to_string()),
        final_path: Some(paths.final_path.to_string_lossy().to_string()),
        total_size,
        downloaded_bytes: 0,
        status: TaskStatus::Downloading,
        etag: None,
        last_modified: None,
        content_type: Some("application/octet-stream".to_string()),
        supports_resume: true,
        supports_parallel,
        supports_multi_file: false,
        source_key: "127.0.0.1".to_string(),
        connection_count: 1,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: Some("Downloading".to_string()),
        error_message: None,
        error_code: None,
        recovery_actions: Vec::new(),
        retry_after_at: None,
        expected_hash_sha256: None,
        actual_hash_sha256: None,
        hash_status: HashVerificationStatus::NotRequested,
        hash_error: None,
        hash_verified_at: None,
        created_at: now.clone(),
        updated_at: now,
        files_version: 0,
    }
}

/// Creates a headless engine context while retaining the production runtime
/// limits, cancellation, proxy, and persistence behavior.
#[allow(dead_code)]
pub fn headless_download_context(
    pool: SqlitePool,
    task: TaskRecord,
    cancel_token: tokio_util::sync::CancellationToken,
) -> DownloadContext {
    DownloadContext {
        app: None,
        pool,
        task,
        cancel_token,
        finish: Arc::new(AtomicBool::new(false)),
        speed_limiter: GlobalSpeedLimiter::disabled(),
        connection_limit: 1,
        request_headers: Vec::new(),
        proxy_config: ResolvedProxyConfig::default(),
    }
}
