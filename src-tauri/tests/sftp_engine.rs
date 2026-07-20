//! F-1 SFTP engine integration tests.
//!
//! Covers the public `SftpEngine::probe()` API surface and the underlying
//! SFTP session operations that the Path A worker coordination loop relies
//! on (per-worker SSH channel + SFTP subsystem). The tests stand up a
//! minimal in-memory SFTP server via `common::sftp_server` and exercise:
//!
//! - Successful probe with `supports_parallel = true`
//! - TOFU host key verification (first-use record + repeat success)
//! - Authentication failure
//! - Host key mismatch (pre-seeded fingerprint differs from server key)
//! - Directory URL rejection
//! - Missing DB pool rejection
//! - Path A concurrent segment reads (independent SSH channels)
//! - Resume from a mid-file offset
//! - Per-session failure isolation (read error in one session does not
//!   corrupt another)
//!
//! The full `DownloadContext` download path (worker coordination loop,
//! dynamic segment splitting, checkpoint persistence) is not exercised
//! here because it requires a `tauri::AppHandle`. That coverage is
//! deferred to Phase 8's cross-engine integration test harness, which
//! will reuse `common::sftp_server`.

mod common;

use std::{
    collections::HashMap,
    io::SeekFrom,
    time::{SystemTime, UNIX_EPOCH},
};

use common::sftp_server::{
    connect_sftp, start_sftp_server, start_sftp_server_with_files, SftpServerConfig,
};
use tauri_app_lib::{
    db,
    download::{DownloadEngine, DownloadError, ProbeRequest, SftpEngine},
    proxy::ResolvedProxyConfig,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

// ===== helpers =====

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-sftp-{label}-{id}.sqlite"));
    db::connect(&path).await.expect("connect").pool
}

fn new_engine() -> SftpEngine {
    SftpEngine::new(ResolvedProxyConfig::shared_default())
}

fn probe_request(uri: String, pool: sqlx::SqlitePool) -> ProbeRequest {
    ProbeRequest {
        uri,
        source: None,
        request_headers: Vec::new(),
        pool: Some(pool),
        task_id: None,
        credentials: None,
        proxy_config: None,
        app: None,
        request_id: None,
    }
}

fn sftp_url(addr: std::net::SocketAddr, path: &str) -> String {
    format!("sftp://u:p@{}:{}/{}", addr.ip(), addr.port(), path)
}

/// Extract the `code` field from a JSON-serialized `AppErrorPayload` error
/// string carried inside `DownloadError::Other`. Returns the raw string if
/// parsing fails.
fn error_code(error: &DownloadError) -> String {
    let DownloadError::Other(message) = error else {
        return error.to_string();
    };
    // The engine layer wraps errors as JSON via AppErrorPayload::command_error().
    // We only need the `code` field for dispatch, so a lightweight substring
    // scan avoids depending on serde_json here.
    let needle = "\"code\":\"";
    if let Some(start) = message.find(needle) {
        let rest = &message[start + needle.len()..];
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    message.clone()
}

/// Insert a fake host key record so the next probe sees a mismatch.
async fn seed_mismatched_host_key(pool: &sqlx::SqlitePool, host: &str, port: u16) {
    let now = tauri_app_lib::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO sftp_known_hosts (host, port, algorithm, fingerprint_sha256, first_seen_at, last_seen_at)
        VALUES (?, ?, 'ssh-ed25519', 'FAKE_MISMATCHED_FINGERPRINT_FOR_TEST', ?, ?)
        "#,
    )
    .bind(host.to_ascii_lowercase())
    .bind(i64::from(port))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("seed mismatched host key");
}

// ===== probe-level integration tests =====

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_succeeds_and_reports_supports_parallel() {
    // Non-empty file → supports_resume = true, supports_parallel = true.
    // This is the core F-1 change: previously SFTP forced supports_parallel
    // = false, capping throughput at a single stream.
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0xA5u8; 4096]);
    let server = start_sftp_server_with_files(files).await;
    let pool = test_pool("probe-parallel").await;
    let engine = new_engine();

    let output = engine
        .probe(probe_request(
            sftp_url(server.addr, "file.bin"),
            pool.clone(),
        ))
        .await
        .expect("probe should succeed");

    assert_eq!(output.protocol, "sftp");
    assert_eq!(output.total_size, 4096);
    assert!(output.capabilities.supports_resume);
    assert!(
        output.capabilities.supports_parallel,
        "SFTP probe must report supports_parallel for non-empty files"
    );
    assert!(!output.capabilities.supports_multi_file);
    assert_eq!(output.files.len(), 1);
    assert_eq!(output.files[0].size, "4096");
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_tofu_first_connection_records_host_key() {
    // TOFU: first connection records the host key, second connection with
    // the same key succeeds. Validates that the probe path exercises
    // `verify_or_record_sftp_host_key` end to end.
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0x11u8; 1024]);
    let server = start_sftp_server_with_files(files).await;
    let pool = test_pool("probe-tofu").await;
    let engine = new_engine();
    let url = sftp_url(server.addr, "file.bin");

    // First probe: records the host key (TOFU).
    engine
        .probe(probe_request(url.clone(), pool.clone()))
        .await
        .expect("first probe should succeed (TOFU record)");

    // Verify the key was recorded and matches the server's actual fingerprint.
    let recorded_fingerprint: String =
        sqlx::query_scalar("SELECT fingerprint_sha256 FROM sftp_known_hosts")
            .fetch_one(&pool)
            .await
            .expect("fetch fingerprint");
    assert_eq!(
        recorded_fingerprint, server.host_key_fingerprint,
        "TOFU must record the server's actual host key fingerprint"
    );

    // Second probe with the same key should also succeed.
    engine
        .probe(probe_request(url, pool.clone()))
        .await
        .expect("second probe should succeed (key matches)");

    // Still only one record (update, not insert) and fingerprint unchanged.
    let (recorded_count, recorded_fingerprint): (i64, String) =
        sqlx::query_as("SELECT COUNT(*), MAX(fingerprint_sha256) FROM sftp_known_hosts")
            .fetch_one(&pool)
            .await
            .expect("count+max");
    assert_eq!(
        recorded_count, 1,
        "repeat connection should update, not insert"
    );
    assert_eq!(
        recorded_fingerprint, server.host_key_fingerprint,
        "fingerprint must be unchanged on repeat connection"
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_on_authentication_failure() {
    // Server rejects all password auth → engine returns sftp_auth_failed.
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0x11u8; 1024]);
    let server = start_sftp_server(SftpServerConfig {
        files,
        reject_auth: true,
        fail_on_read: None,
        stall_on_read: false,
        read_chunk_delay: None,
        deny_open: false,
    })
    .await;
    let pool = test_pool("probe-auth-fail").await;
    let engine = new_engine();

    let error = engine
        .probe(probe_request(
            sftp_url(server.addr, "file.bin"),
            pool.clone(),
        ))
        .await
        .expect_err("probe should fail on auth rejection");

    assert_eq!(error_code(&error), "sftp_auth_failed");
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_on_host_key_mismatch() {
    // Pre-seed sftp_known_hosts with a fake fingerprint that differs from
    // the server's real key. The probe should detect the mismatch and
    // return sftp_host_key_changed (which is non-retryable).
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0x22u8; 2048]);
    let server = start_sftp_server_with_files(files).await;
    let pool = test_pool("probe-hostkey-mismatch").await;
    seed_mismatched_host_key(&pool, &server.addr.ip().to_string(), server.addr.port()).await;
    let engine = new_engine();

    let error = engine
        .probe(probe_request(
            sftp_url(server.addr, "file.bin"),
            pool.clone(),
        ))
        .await
        .expect_err("probe should fail on host key mismatch");

    assert_eq!(error_code(&error), "sftp_host_key_changed");
    // The mismatched record must not be overwritten — the user must
    // explicitly clear it before retrying.
    let still_present: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sftp_known_hosts")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(
        still_present, 1,
        "mismatched record must remain for user review"
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn arc15_list_and_forget_known_host_then_retofu() {
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0x44u8; 1024]);
    let server = start_sftp_server_with_files(files).await;
    let pool = test_pool("arc15-list-forget").await;
    let host = server.addr.ip().to_string();
    let port = server.addr.port();
    seed_mismatched_host_key(&pool, &host, port).await;

    let listed = db::list_sftp_known_hosts(&pool).await.expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].host, host.to_ascii_lowercase());
    assert_eq!(listed[0].port, port);
    assert_ne!(listed[0].fingerprint_sha256, server.host_key_fingerprint);

    // Mismatch must still fail closed and must not overwrite the row.
    let engine = new_engine();
    let error = engine
        .probe(probe_request(
            sftp_url(server.addr, "file.bin"),
            pool.clone(),
        ))
        .await
        .expect_err("probe should fail on host key mismatch");
    assert_eq!(error_code(&error), "sftp_host_key_changed");
    let DownloadError::Other(message) = &error else {
        panic!("expected Other");
    };
    assert!(
        message.contains("manage_sftp_host_keys") && message.contains("retry"),
        "recovery actions should include manage_sftp_host_keys and retry: {message}"
    );

    assert!(
        db::forget_sftp_known_host(&pool, &host.to_ascii_uppercase(), port)
            .await
            .expect("forget"),
        "forget should delete the row (host lowercased)"
    );
    assert!(db::list_sftp_known_hosts(&pool)
        .await
        .expect("list empty")
        .is_empty());

    // After explicit forget, TOFU INSERT succeeds with the live fingerprint.
    engine
        .probe(probe_request(
            sftp_url(server.addr, "file.bin"),
            pool.clone(),
        ))
        .await
        .expect("probe after forget");
    let after = db::list_sftp_known_hosts(&pool).await.expect("list after");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].fingerprint_sha256, server.host_key_fingerprint);

    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_directory_url_returns_error() {
    // Probing a directory URL (path ends with `/`) should be rejected at
    // URL parse time before any network call is made.
    let server = start_sftp_server_with_files(HashMap::new()).await;
    let pool = test_pool("probe-directory").await;
    let engine = new_engine();

    let error = engine
        .probe(probe_request(
            format!(
                "sftp://u:p@{}:{}/dir/",
                server.addr.ip(),
                server.addr.port()
            ),
            pool.clone(),
        ))
        .await
        .expect_err("probe should reject directory URL");

    assert_eq!(error_code(&error), "sftp_directory_not_file");
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_fails_without_db_pool() {
    // SFTP probe requires a DB pool for host key verification. Omitting it
    // should produce sftp_probe_state_unavailable rather than a panic.
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0x33u8; 512]);
    let server = start_sftp_server_with_files(files).await;
    let engine = new_engine();

    let request = ProbeRequest {
        uri: sftp_url(server.addr, "file.bin"),
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
        proxy_config: None,
        app: None,
        request_id: None,
    };

    let error = engine
        .probe(request)
        .await
        .expect_err("probe should fail without DB pool");

    assert_eq!(error_code(&error), "sftp_probe_state_unavailable");
}

// ===== Path A session-level tests =====
//
// These tests exercise the underlying SFTP operations that the Path A
// worker coordination loop (`download_sftp_segment_inner`) relies on:
// each worker opens its own SSH channel + SFTP session and reads its
// segment range independently. We validate this directly at the session
// level because the full `download()` path requires an `AppHandle`.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_a_concurrent_segment_reads_produce_correct_bytes() {
    // Mirrors the Path A design: each worker opens its OWN SSH channel +
    // SFTP session and reads its byte range independently. This is the
    // pattern `download_sftp_segment_inner` uses (one `connect_sftp` per
    // worker). Validates that two concurrent sessions reading different
    // ranges of the same file produce correct, non-overlapping bytes.
    let total_size = 64 * 1024;
    let segment_size = 32 * 1024;
    let mut content = Vec::with_capacity(total_size);
    for i in 0..total_size {
        content.push((i % 256) as u8);
    }
    let mut files = HashMap::new();
    files.insert("/large.bin".to_string(), content.clone());
    let server = start_sftp_server_with_files(files).await;

    // Two independent sessions, each reading its own 32 KB segment.
    let (session_a, session_b) = tokio::join!(
        connect_sftp(server.addr, "u", "p"),
        connect_sftp(server.addr, "u", "p"),
    );

    let (result_a, result_b) = tokio::join!(
        read_segment(&session_a, "/large.bin", 0, segment_size),
        read_segment(&session_b, "/large.bin", segment_size as u64, segment_size),
    );

    let bytes_a = result_a.expect("segment A read");
    let bytes_b = result_b.expect("segment B read");
    assert_eq!(bytes_a.len(), segment_size);
    assert_eq!(bytes_b.len(), segment_size);
    assert_eq!(&bytes_a[..], &content[0..segment_size], "segment A content");
    assert_eq!(
        &bytes_b[..],
        &content[segment_size..total_size],
        "segment B content"
    );

    let _ = session_a.close().await;
    let _ = session_b.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn path_a_resume_continues_from_offset() {
    // Validates resume: open a file, seek to a mid-file offset, read the
    // remainder. This is the exact pattern `download_sftp_segment_inner`
    // uses when `segment.downloaded_until > range_start` (resume after a
    // pause or worker restart).
    let total_size = 16 * 1024;
    let resume_offset = 4 * 1024; // pretend 4 KB was already downloaded
    let mut content = Vec::with_capacity(total_size);
    for i in 0..total_size {
        content.push((i % 256) as u8);
    }
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), content.clone());
    let server = start_sftp_server_with_files(files).await;
    let session = connect_sftp(server.addr, "u", "p").await;

    let mut file = session.open("/file.bin").await.expect("open");
    file.seek(SeekFrom::Start(u64::try_from(resume_offset).unwrap()))
        .await
        .expect("seek to resume offset");

    let mut buf = vec![0u8; total_size - resume_offset];
    let n = file.read(&mut buf).await.expect("read from offset");
    assert_eq!(n, total_size - resume_offset, "should read remaining bytes");
    assert_eq!(
        &buf[..n],
        &content[resume_offset..total_size],
        "resumed content must match source"
    );

    let _ = session.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_read_failure_isolates_per_session() {
    // Validates that a read failure in one SSH session does NOT corrupt
    // reads in a separate session. This is the key Path A isolation
    // property: per-worker channels mean a server-side error on one
    // connection cannot block or corrupt another worker's transfer.
    //
    // The test configures the server to fail on the first read call, then
    // opens two sessions concurrently. The first session to read hits the
    // injected failure; the second session's read should still succeed.
    // (Session ordering is non-deterministic, but exactly one read fails
    // and exactly one succeeds.)
    let total_size = 8 * 1024;
    let mut content = Vec::with_capacity(total_size);
    for i in 0..total_size {
        content.push((i % 256) as u8);
    }
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), content.clone());
    let server = start_sftp_server(SftpServerConfig {
        files,
        reject_auth: false,
        fail_on_read: Some(0), // first read call fails
        stall_on_read: false,
        read_chunk_delay: None,
        deny_open: false,
    })
    .await;

    let (session_a, session_b) = tokio::join!(
        connect_sftp(server.addr, "u", "p"),
        connect_sftp(server.addr, "u", "p"),
    );

    let (result_a, result_b) = tokio::join!(
        read_segment(&session_a, "/file.bin", 0, total_size),
        read_segment(&session_b, "/file.bin", 0, total_size),
    );

    // Exactly one must fail (permission denied) and the other must succeed.
    let a_failed = result_a.is_err();
    let b_failed = result_b.is_err();
    assert!(
        a_failed ^ b_failed,
        "exactly one session should fail (a_failed={a_failed}, b_failed={b_failed}); \
         Path A isolation means a read error on one channel cannot break the other"
    );

    let _ = session_a.close().await;
    let _ = session_b.close().await;
}

/// Helper: open a file, seek to `offset`, read `len` bytes. Returns the
/// bytes read. Used by the Path A session-level tests.
async fn read_segment(
    session: &russh_sftp::client::SftpSession,
    path: &str,
    offset: u64,
    len: usize,
) -> Result<Vec<u8>, String> {
    let mut file = session.open(path).await.map_err(|e| e.to_string())?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))
            .await
            .map_err(|e| e.to_string())?;
    }
    let mut buf = vec![0u8; len];
    let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
    buf.truncate(n);
    Ok(buf)
}

// ===== E-1 idle-read timeout integration =====

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sftp_stalled_read_is_detectable_via_idle_timeout() {
    // E-1: `download_sftp_segment_inner` wraps `remote.read()` with
    // `read_with_idle_timeout(READ_IDLE_TIMEOUT = 60s)`. A stalled SFTP
    // server (read never responds) would trigger `sftp_read_timeout` in
    // production. This test verifies the stall scenario at the session
    // level: the SFTP `file.read()` future hangs indefinitely when the
    // server's `stall_on_read` flag is set, and a short
    // `tokio::time::timeout` wrapper detects it. The production helper
    // (`read_with_idle_timeout`) is the same pattern with a 60s timeout;
    // its 4 branches (Data/End/Error/IdleTimeout) are covered by unit
    // tests in `src/download/mod.rs`.
    //
    // We verify at 500ms rather than 60s to keep the test fast. The
    // assertion is that the read future is still pending after 500ms,
    // proving the stall is real and would be caught by the idle-timeout
    // wrapper in production.
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![0xA5u8; 4096]);
    let server = start_sftp_server(SftpServerConfig {
        files,
        reject_auth: false,
        fail_on_read: None,
        stall_on_read: true,
        read_chunk_delay: None,
        deny_open: false,
    })
    .await;

    let session = connect_sftp(server.addr, "u", "p").await;
    let mut file = session.open("/file.bin").await.expect("open");

    let mut buf = vec![0u8; 1024];
    // The read should hang because the server never responds.
    let result =
        tokio::time::timeout(std::time::Duration::from_millis(500), file.read(&mut buf)).await;

    assert!(
        result.is_err(),
        "SFTP read on a stalled server must hang; if it resolved, the \
         stall_on_read injection is broken and the E-1 idle-timeout path \
         cannot be exercised"
    );

    let _ = session.close().await;
}

// ===== C4: credentials, permission, host-key download, restart =====

async fn seed_matching_host_key(pool: &sqlx::SqlitePool, host: &str, port: u16, fingerprint: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        r#"
        INSERT INTO sftp_known_hosts (host, port, algorithm, fingerprint_sha256, first_seen_at, last_seen_at)
        VALUES (?, ?, 'ssh-ed25519', ?, ?, ?)
        "#,
    )
    .bind(host.to_ascii_lowercase())
    .bind(i64::from(port))
    .bind(fingerprint)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .expect("seed known host");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_uses_persisted_sftp_credentials() {
    common::install_test_secret_key();
    let payload = b"sftp-cred-payload".to_vec();
    let mut files = HashMap::new();
    files.insert("/protected.bin".to_string(), payload.clone());
    let server = start_sftp_server_with_files(files).await;
    let pool = common::test_pool("sftp-cred-rotation").await;
    seed_matching_host_key(
        &pool,
        &server.addr.ip().to_string(),
        server.addr.port(),
        &server.host_key_fingerprint,
    )
    .await;

    let paths = common::TestPaths::new("sftp-cred-rotation");
    let url = format!(
        "sftp://{}:{}/protected.bin",
        server.addr.ip(),
        server.addr.port()
    );
    let task = common::download_task(
        "sftp-cred-rotation",
        url,
        "sftp",
        "protected.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(&pool, &task.id, "sftp", "sftpuser", "sftppass", None, None)
        .await
        .expect("store SFTP credentials");

    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("authenticated SFTP download");

    assert_eq!(
        std::fs::read(&paths.final_path).expect("read final"),
        payload
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_uses_persisted_sftp_private_key_credentials() {
    common::install_test_secret_key();
    let payload = b"sftp-key-payload".to_vec();
    let mut files = HashMap::new();
    files.insert("/key-protected.bin".to_string(), payload.clone());
    let server = start_sftp_server_with_files(files).await;
    let pool = common::test_pool("sftp-key-cred-rotation").await;
    seed_matching_host_key(
        &pool,
        &server.addr.ip().to_string(),
        server.addr.port(),
        &server.host_key_fingerprint,
    )
    .await;

    let key = russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
        .expect("client key");
    let key_pem = key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .expect("pem")
        .to_string();

    let paths = common::TestPaths::new("sftp-key-cred-rotation");
    let url = format!(
        "sftp://{}:{}/key-protected.bin",
        server.addr.ip(),
        server.addr.port()
    );
    let task = common::download_task(
        "sftp-key-cred-rotation",
        url,
        "sftp",
        "key-protected.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(
        &pool,
        &task.id,
        "sftp",
        "sftpuser",
        "",
        Some(key_pem.as_str()),
        None,
    )
    .await
    .expect("store SFTP private-key credentials");

    new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("private-key authenticated SFTP download");

    assert_eq!(
        std::fs::read(&paths.final_path).expect("read final"),
        payload
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_fails_on_permission_denied_open() {
    common::install_test_secret_key();
    let mut files = HashMap::new();
    files.insert("/denied.bin".to_string(), vec![0u8; 64]);
    let server = start_sftp_server(SftpServerConfig {
        files,
        deny_open: true,
        ..Default::default()
    })
    .await;
    let pool = common::test_pool("sftp-perm-denied").await;
    seed_matching_host_key(
        &pool,
        &server.addr.ip().to_string(),
        server.addr.port(),
        &server.host_key_fingerprint,
    )
    .await;
    let paths = common::TestPaths::new("sftp-perm-denied");
    let url = format!(
        "sftp://{}:{}/denied.bin",
        server.addr.ip(),
        server.addr.port()
    );
    let task = common::download_task(
        "sftp-perm-denied",
        url,
        "sftp",
        "denied.bin",
        64,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(&pool, &task.id, "sftp", "u", "p", None, None)
        .await
        .expect("store credentials");

    let error = new_engine()
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("permission denied must fail download");
    assert_eq!(error_code(&error), "sftp_permission_denied");
    assert!(!paths.final_path.exists());
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_host_key_mismatch_then_forget_and_retry() {
    common::install_test_secret_key();
    let payload = b"after-forget".to_vec();
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), payload.clone());
    let server = start_sftp_server_with_files(files).await;
    let pool = common::test_pool("sftp-hostkey-download").await;
    let host = server.addr.ip().to_string();
    let port = server.addr.port();
    seed_mismatched_host_key(&pool, &host, port).await;

    let paths = common::TestPaths::new("sftp-hostkey-download");
    let url = format!("sftp://{host}:{port}/file.bin");
    let task = common::download_task(
        "sftp-hostkey-download",
        url,
        "sftp",
        "file.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(&pool, &task.id, "sftp", "u", "p", None, None)
        .await
        .expect("store credentials");

    let engine = new_engine();
    let error = engine
        .download(common::headless_download_context(
            pool.clone(),
            task.clone(),
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("host-key mismatch must fail closed");
    assert_eq!(error_code(&error), "sftp_host_key_changed");
    let DownloadError::Other(message) = &error else {
        panic!("expected Other");
    };
    assert!(
        message.contains("manage_sftp_host_keys") && message.contains("retry"),
        "recovery actions missing: {message}"
    );

    assert!(
        db::forget_sftp_known_host(&pool, &host, port)
            .await
            .expect("forget"),
        "forget should delete mismatched row"
    );

    engine
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("download after forget should TOFU and succeed");
    assert_eq!(
        std::fs::read(&paths.final_path).expect("read final"),
        payload
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn download_pauses_mid_transfer_and_resumes_from_persisted_offset() {
    common::install_test_secret_key();
    let payload: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let mut files = HashMap::new();
    files.insert("/resume.bin".to_string(), payload.clone());
    let server = start_sftp_server(SftpServerConfig {
        files,
        read_chunk_delay: Some(std::time::Duration::from_millis(50)),
        ..Default::default()
    })
    .await;
    let pool = common::test_pool("sftp-pause-resume").await;
    seed_matching_host_key(
        &pool,
        &server.addr.ip().to_string(),
        server.addr.port(),
        &server.host_key_fingerprint,
    )
    .await;
    let paths = common::TestPaths::new("sftp-pause-resume");
    let url = format!(
        "sftp://{}:{}/resume.bin",
        server.addr.ip(),
        server.addr.port()
    );
    let task = common::download_task(
        "sftp-pause-resume",
        url,
        "sftp",
        "resume.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(&pool, &task.id, "sftp", "u", "p", None, None)
        .await
        .expect("store credentials");

    let engine = new_engine();
    let cancel = tokio_util::sync::CancellationToken::new();
    let first = tokio::spawn({
        let engine = engine.clone();
        let context = common::headless_download_context(pool.clone(), task.clone(), cancel.clone());
        async move { engine.download(context).await }
    });

    let partial = loop {
        let segments = db::list_segment_records(&pool, "sftp-pause-resume")
            .await
            .expect("list segments");
        if let Some(downloaded) = segments
            .first()
            .map(|segment| segment.downloaded_until)
            .filter(|downloaded| *downloaded > 0)
        {
            break downloaded;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    };
    assert!(partial < payload.len() as i64);
    cancel.cancel();
    first
        .await
        .expect("join")
        .expect("SFTP cancellation is a clean pause boundary");

    let current = db::get_task_record(&pool, "sftp-pause-resume")
        .await
        .expect("read")
        .expect("exists");
    let no_app = Option::<tauri::AppHandle>::None;
    tauri_app_lib::state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &current.id,
        tauri_app_lib::models::TaskStatus::Paused,
        current.downloaded_bytes,
        0,
        Some("Paused"),
        Some("paused"),
        None,
        tauri_app_lib::models::SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("persist pause");
    let resumed = tauri_app_lib::state_machine::transition_task_with_runtime_state(
        &no_app,
        &pool,
        &current.id,
        tauri_app_lib::models::TaskStatus::Downloading,
        current.downloaded_bytes,
        1,
        Some("Downloading"),
        Some("resumed"),
        None,
        tauri_app_lib::models::SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("persist resume");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            resumed,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("resume download");

    assert_eq!(
        std::fs::read(&paths.final_path).expect("read final"),
        payload
    );
    assert!(!paths.temp.exists());
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_fails_when_socks5_proxy_is_unreachable() {
    common::install_test_secret_key();
    let mut files = HashMap::new();
    files.insert("/via-proxy.bin".to_string(), b"x".to_vec());
    let server = start_sftp_server_with_files(files).await;
    let pool = common::test_pool("sftp-proxy-fail").await;
    seed_matching_host_key(
        &pool,
        &server.addr.ip().to_string(),
        server.addr.port(),
        &server.host_key_fingerprint,
    )
    .await;
    let paths = common::TestPaths::new("sftp-proxy-fail");
    let url = format!(
        "sftp://{}:{}/via-proxy.bin",
        server.addr.ip(),
        server.addr.port()
    );
    let task = common::download_task(
        "sftp-proxy-fail",
        url,
        "sftp",
        "via-proxy.bin",
        1,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(&pool, &task.id, "sftp", "u", "p", None, None)
        .await
        .expect("store credentials");

    let mut context = common::headless_download_context(
        pool.clone(),
        task,
        tokio_util::sync::CancellationToken::new(),
    );
    context.proxy_config = ResolvedProxyConfig {
        mode: tauri_app_lib::proxy::AppProxyMode::Custom,
        url: Some("socks5://127.0.0.1:1".into()),
        no_proxy: None,
        username: None,
        password: None,
    };

    let error = new_engine()
        .download(context)
        .await
        .expect_err("unreachable SOCKS5 must fail SFTP download");
    let code = error_code(&error);
    assert!(
        code == "proxy_connection_failed" || code.contains("proxy"),
        "expected proxy failure code, got: {code}"
    );
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_rejects_non_socks5_custom_proxy() {
    common::install_test_secret_key();
    let mut files = HashMap::new();
    files.insert("/file.bin".to_string(), vec![1u8; 8]);
    let server = start_sftp_server_with_files(files).await;
    let pool = common::test_pool("sftp-proxy-unsupported").await;
    seed_matching_host_key(
        &pool,
        &server.addr.ip().to_string(),
        server.addr.port(),
        &server.host_key_fingerprint,
    )
    .await;
    let paths = common::TestPaths::new("sftp-proxy-unsupported");
    let url = format!(
        "sftp://{}:{}/file.bin",
        server.addr.ip(),
        server.addr.port()
    );
    let task = common::download_task(
        "sftp-proxy-unsupported",
        url,
        "sftp",
        "file.bin",
        8,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert SFTP task");
    db::upsert_task_credentials(&pool, &task.id, "sftp", "u", "p", None, None)
        .await
        .expect("store credentials");

    let mut context = common::headless_download_context(
        pool.clone(),
        task,
        tokio_util::sync::CancellationToken::new(),
    );
    // Probe uses the engine SharedProxyConfig; download uses context.proxy_config.
    context.proxy_config = ResolvedProxyConfig {
        mode: tauri_app_lib::proxy::AppProxyMode::Custom,
        url: Some("http://127.0.0.1:8080".into()),
        no_proxy: None,
        username: None,
        password: None,
    };

    let error = new_engine()
        .download(context)
        .await
        .expect_err("HTTP custom proxy must be rejected for SFTP");
    assert_eq!(error_code(&error), "sftp_proxy_unsupported");
    pool.close().await;
}
