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
        .probe(probe_request(sftp_url(server.addr, "file.bin"), pool.clone()))
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
    assert_eq!(recorded_count, 1, "repeat connection should update, not insert");
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
    })
    .await;
    let pool = test_pool("probe-auth-fail").await;
    let engine = new_engine();

    let error = engine
        .probe(probe_request(sftp_url(server.addr, "file.bin"), pool.clone()))
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
        .probe(probe_request(sftp_url(server.addr, "file.bin"), pool.clone()))
        .await
        .expect_err("probe should fail on host key mismatch");

    assert_eq!(error_code(&error), "sftp_host_key_changed");
    // The mismatched record must not be overwritten — the user must
    // explicitly clear it before retrying.
    let still_present: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sftp_known_hosts")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(still_present, 1, "mismatched record must remain for user review");
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
            format!("sftp://u:p@{}:{}/dir/", server.addr.ip(), server.addr.port()),
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
    })
    .await;

    let session = connect_sftp(server.addr, "u", "p").await;
    let mut file = session.open("/file.bin").await.expect("open");

    let mut buf = vec![0u8; 1024];
    // The read should hang because the server never responds.
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        file.read(&mut buf),
    )
    .await;

    assert!(
        result.is_err(),
        "SFTP read on a stalled server must hang; if it resolved, the \
         stall_on_read injection is broken and the E-1 idle-timeout path \
         cannot be exercised"
    );

    let _ = session.close().await;
}
