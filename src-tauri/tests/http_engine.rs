mod common;

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::TcpStream,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use common::{TestPaths, TestServer};
use sha2::{Digest, Sha256};
use tauri_app_lib::{
    db,
    download::GlobalSpeedLimiter,
    download::{DirectDownloadRequest, DirectSegmentedDownloadRequest, HttpEngine},
    models::{SegmentStatus, TaskSegmentRecord},
};

const SAMPLE: &[u8] = b"Vibe Downloader HTTP regression payload.";
const LARGE_PAYLOAD_SHA256: &str =
    "f1808c3366e106973e30f4fa360e5355f36284aa0f299705ef9ee0a0d9648fc3";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_reads_headers_and_range_support() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/file", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.file_name, "sample.bin");
    assert_eq!(probe.total_size, SAMPLE.len() as i64);
    assert!(probe.supports_parallel);
    assert_eq!(probe.source_key, "127.0.0.1");
    assert_eq!(
        probe.content_type.as_deref(),
        Some("application/octet-stream")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_falls_back_to_get_range_when_head_is_incomplete() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/head-no-length", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.file_name, "fallback.bin");
    assert_eq!(probe.total_size, SAMPLE.len() as i64);
    assert!(probe.supports_parallel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_sends_identity_accept_encoding() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/requires-identity", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.file_name, "identity.bin");
    assert!(probe.supports_parallel);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_allows_unknown_size_single_streams() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/unknown-size", server.base_url))
        .await
        .expect("probe");

    assert_eq!(probe.total_size, 0);
    assert!(!probe.supports_parallel);
    assert_eq!(probe.file_name, "unknown-size.bin");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_size_range_probe_captures_validator_and_resumes_safely() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/unknown-range-stable", server.base_url);
    let probe = engine.probe(&url).await.expect("unknown-size probe");

    assert_eq!(probe.total_size, 0);
    assert!(probe.supports_resume);
    assert_eq!(probe.etag.as_deref(), Some("\"stable\""));

    let paths = TestPaths::new("unknown-range-resume");
    let prefix = &SAMPLE[..7];
    fs::write(&paths.temp, prefix).expect("write partial file");
    engine
        .download_direct(
            DirectDownloadRequest {
                url,
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: probe.total_size,
                supports_resume: probe.supports_resume,
                supports_parallel: probe.supports_parallel,
                etag: probe.etag,
                last_modified: probe.last_modified,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("resume unknown-size resource");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_size_probe_and_resume_reject_unverifiable_ranges() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let invalid_probe = engine
        .probe(&format!("{}/unknown-range-invalid-probe", server.base_url))
        .await
        .expect("probe invalid range response");
    assert!(!invalid_probe.supports_resume);
    let weak_probe = engine
        .probe(&format!("{}/unknown-range-weak-validator", server.base_url))
        .await
        .expect("probe weak ETag response");
    assert!(!weak_probe.supports_resume);

    for (label, path, expected_error) in [
        (
            "unknown-range-changed-validator",
            "/unknown-range-changed-validator",
            "Resume unavailable. The remote resource validator changed during resume.",
        ),
        (
            "unknown-range-missing-validator",
            "/unknown-range-missing-validator",
            "Resume unavailable. The remote resource validator changed during resume.",
        ),
        (
            "unknown-range-invalid-resume",
            "/unknown-range-invalid-resume",
            "Resume unavailable. The server returned a mismatched Content-Range.",
        ),
    ] {
        let paths = TestPaths::new(label);
        let prefix = &SAMPLE[..7];
        fs::write(&paths.temp, prefix).expect("write partial file");
        let error = engine
            .download_direct(
                DirectDownloadRequest {
                    url: format!("{}{}", server.base_url, path),
                    temp_path: paths.temp.clone(),
                    final_path: paths.final_path.clone(),
                    total_size: 0,
                    supports_resume: true,
                    supports_parallel: false,
                    etag: Some("\"stable\"".to_string()),
                    last_modified: None,
                },
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect_err("unsafe unknown-size resume must fail");

        assert_eq!(error, expected_error, "route {path}");
        assert_eq!(fs::read(&paths.temp).expect("preserved partial"), prefix);
        assert!(!paths.final_path.exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_unknown_size_body_fails_without_retryable_transport_code() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/unknown-malformed-chunk", server.base_url);
    let probe = engine
        .probe(&url)
        .await
        .expect("probe malformed-body fixture");
    assert_eq!(probe.total_size, 0);
    assert!(probe.supports_resume);

    let (_db, pool) = common::test_pool("unknown-size-malformed-body").await;
    let paths = TestPaths::new("unknown-size-malformed-body");
    let mut task = common::download_task(
        "unknown-size-malformed-body",
        url,
        "http",
        "unknown.bin",
        probe.total_size,
        &paths,
        false,
    );
    task.supports_resume = probe.supports_resume;
    task.etag = probe.etag;
    task.last_modified = probe.last_modified;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let error = engine
        .download(common::headless_download_context(
            pool,
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("malformed chunk framing must fail");
    let payload: tauri_app_lib::models::AppErrorPayload =
        serde_json::from_str(&error).expect("structured malformed-body error");
    assert!(matches!(
        payload.code.as_str(),
        "decode_error" | "body_error"
    ));
    assert!(!payload.recoverable);
    assert!(!paths.final_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_size_legacy_weak_etag_restarts_from_byte_zero() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/unknown-range-weak-validator", server.base_url);
    let (_db, pool) = common::test_pool("unknown-size-weak-etag-restart").await;
    let paths = TestPaths::new("unknown-size-weak-etag-restart");
    fs::write(&paths.temp, b"stale").expect("write stale prefix");
    let mut task = common::download_task(
        "unknown-size-weak-etag-restart",
        url,
        "http",
        "unknown.bin",
        0,
        &paths,
        false,
    );
    // Older task rows can claim resume support even though a weak ETag is not
    // a byte-identity validator; runtime validation must still fail closed.
    task.supports_resume = true;
    task.etag = Some("W/\"weak\"".to_string());
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert legacy task");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            task.clone(),
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("restart weak-validator task from zero");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
    let completed = db::get_task_record(&pool, &task.id)
        .await
        .expect("load completed task")
        .expect("task exists");
    assert_eq!(completed.downloaded_bytes, SAMPLE.len() as i64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_size_task_retry_resumes_after_a_midstream_disconnect() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/unknown-resume-after-reset", server.base_url);
    let probe = engine.probe(&url).await.expect("unknown-size probe");
    assert!(probe.supports_resume);
    assert_eq!(probe.etag.as_deref(), Some("\"stable\""));

    let (_db, pool) = common::test_pool("unknown-size-auto-retry").await;
    let paths = TestPaths::new("unknown-size-auto-retry");
    let payload = slow_resume_payload();
    let mut task = common::download_task(
        "unknown-size-auto-retry",
        url,
        "http",
        "unknown.bin",
        probe.total_size,
        &paths,
        false,
    );
    task.supports_resume = probe.supports_resume;
    task.etag = probe.etag;
    task.last_modified = probe.last_modified;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let first_error = engine
        .download(common::headless_download_context(
            pool.clone(),
            task.clone(),
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("first body should disconnect");
    let first_payload: tauri_app_lib::models::AppErrorPayload =
        serde_json::from_str(&first_error).expect("structured transport failure");
    assert_eq!(first_payload.code, "transport_interrupted");
    assert!(first_payload.recoverable);
    let partial_size = fs::metadata(&paths.temp)
        .expect("temp file after disconnect")
        .len();
    assert!(partial_size > 0, "some body bytes must be durable");
    assert!(partial_size < payload.len() as u64);

    let retry = db::schedule_auto_retry(
        &pool,
        &task.id,
        1,
        "2000-01-01T00:00:00Z",
        "transport_interrupted",
        &first_error,
    )
    .await
    .expect("persist automatic retry");
    assert_eq!(retry, db::AutoRetryOutcome::Scheduled { attempt: 1 });
    let queued = db::get_task_record(&pool, &task.id)
        .await
        .expect("load queued task")
        .expect("task exists");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            queued,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("resume after automatic retry");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), payload);
    let completed = db::get_task_record(&pool, &task.id)
        .await
        .expect("load completed task")
        .expect("task exists");
    assert_eq!(
        completed.status,
        tauri_app_lib::models::TaskStatus::Completed
    );
    assert_eq!(completed.downloaded_bytes, payload.len() as i64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_size_task_retry_restarts_when_range_resume_is_unavailable() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/unknown-restart-after-reset", server.base_url);
    let probe = engine.probe(&url).await.expect("unknown-size probe");
    assert_eq!(probe.total_size, 0);
    assert!(!probe.supports_resume);
    assert!(probe.etag.is_none());

    let (_db, pool) = common::test_pool("unknown-size-restart-retry").await;
    let paths = TestPaths::new("unknown-size-restart-retry");
    let payload = slow_resume_payload();
    let mut task = common::download_task(
        "unknown-size-restart-retry",
        url,
        "http",
        "unknown.bin",
        probe.total_size,
        &paths,
        false,
    );
    task.supports_resume = probe.supports_resume;
    task.etag = probe.etag;
    task.last_modified = probe.last_modified;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    let first_error = engine
        .download(common::headless_download_context(
            pool.clone(),
            task.clone(),
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect_err("first body should disconnect");
    let first_payload: tauri_app_lib::models::AppErrorPayload =
        serde_json::from_str(&first_error).expect("structured transport failure");
    assert_eq!(first_payload.code, "transport_interrupted");
    assert!(first_payload.recoverable);
    assert!(fs::metadata(&paths.temp).expect("partial temp file").len() > 0);

    db::schedule_auto_retry(
        &pool,
        &task.id,
        1,
        "2000-01-01T00:00:00Z",
        "transport_interrupted",
        &first_error,
    )
    .await
    .expect("persist automatic retry");
    let queued = db::get_task_record(&pool, &task.id)
        .await
        .expect("load queued task")
        .expect("task exists");

    engine
        .download(common::headless_download_context(
            pool.clone(),
            queued,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("restart unsupported resume from byte zero");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), payload);
    let completed = db::get_task_record(&pool, &task.id)
        .await
        .expect("load completed task")
        .expect("task exists");
    assert_eq!(
        completed.status,
        tauri_app_lib::models::TaskStatus::Completed
    );
    assert_eq!(completed.downloaded_bytes, payload.len() as i64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_uses_extended_file_name_sources() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let content_location = engine
        .probe(&format!("{}/content-location-name", server.base_url))
        .await
        .expect("content location probe");
    let query_name = engine
        .probe(&format!(
            "{}/query-name?response-content-disposition=attachment%3B%20filename%3D%22query.zip%22",
            server.base_url
        ))
        .await
        .expect("query probe");
    let encoded_name = engine
        .probe(&format!("{}/encoded-name", server.base_url))
        .await
        .expect("encoded probe");

    assert_eq!(content_location.file_name, "report.pdf");
    assert_eq!(query_name.file_name, "query.zip");
    assert_eq!(encoded_name.file_name, "encoded name.txt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_maps_common_http_failures() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let not_found = engine
        .probe(&format!("{}/status/404", server.base_url))
        .await
        .expect_err("404 should fail");
    let denied = engine
        .probe(&format!("{}/status/403", server.base_url))
        .await
        .expect_err("403 should fail");
    let limited = engine
        .probe(&format!("{}/status/429", server.base_url))
        .await
        .expect_err("429 should fail");

    let not_found: serde_json::Value = serde_json::from_str(&not_found).expect("404 payload");
    let denied: serde_json::Value = serde_json::from_str(&denied).expect("403 payload");
    let limited: serde_json::Value = serde_json::from_str(&limited).expect("429 payload");

    assert_eq!(not_found["code"], "http_not_found");
    assert_eq!(
        not_found["message"],
        "The file was not found on the server."
    );
    assert_eq!(denied["code"], "http_denied");
    assert_eq!(denied["message"], "The server denied access to this file.");
    assert_eq!(limited["code"], "server_rate_limited");
    assert_eq!(
        limited["message"],
        "The server is limiting requests. Try again later."
    );
    assert_eq!(limited["recoverable"], true);
    assert!(limited.get("retryAfterAt").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_preserves_retry_after_delta_and_http_date_deadlines() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let delta_error = engine
        .probe(&format!("{}/status/429-retry-seconds", server.base_url))
        .await
        .expect_err("429 should fail");
    let delta: serde_json::Value = serde_json::from_str(&delta_error).expect("delta payload");
    let delta_at = chrono::DateTime::parse_from_rfc3339(
        delta["retryAfterAt"].as_str().expect("delta deadline"),
    )
    .expect("parse delta deadline");
    let delta_remaining = delta_at.with_timezone(&chrono::Utc) - chrono::Utc::now();
    assert!(delta_remaining >= chrono::Duration::seconds(44));
    assert!(delta_remaining <= chrono::Duration::seconds(45));

    let date_error = engine
        .probe(&format!("{}/status/429-retry-date", server.base_url))
        .await
        .expect_err("429 HTTP-date should fail");
    let date: serde_json::Value = serde_json::from_str(&date_error).expect("date payload");
    let retry_after_at = date["retryAfterAt"].as_str().expect("HTTP-date deadline");
    let parsed = chrono::DateTime::parse_from_rfc3339(retry_after_at).expect("parse date");
    assert_eq!(parsed.to_rfc3339(), "2030-01-02T03:04:05+00:00");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn download_uses_persisted_basic_auth_credentials() {
    // FUN-01: credentials stored at create must authorize the real download path.
    common::install_test_secret_key();
    let server = start_test_server();
    let (_db, pool) = common::test_pool("http-basic-auth").await;
    let paths = TestPaths::new("http-basic-auth");
    let url = format!("{}/basic-auth", server.base_url);
    let task = common::download_task(
        "http-basic-auth",
        url,
        "http",
        "protected.bin",
        SAMPLE.len() as i64,
        &paths,
        true,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");
    db::upsert_task_credentials(&pool, &task.id, "http", "user", "pass", None, None)
        .await
        .expect("store credentials");

    let engine = HttpEngine::new().expect("engine");
    engine
        .download(common::headless_download_context(
            pool.clone(),
            task,
            tokio_util::sync::CancellationToken::new(),
        ))
        .await
        .expect("authenticated download");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
    // Authorization must not be written into the persisted request-headers table.
    let header_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_request_headers WHERE task_id = ?")
            .bind("http-basic-auth")
            .fetch_one(&pool)
            .await
            .expect("query headers");
    assert_eq!(header_count, 0, "Authorization must not be persisted");
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_writes_final_file() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("complete");

    let downloaded = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/file", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("download");

    assert_eq!(downloaded, SAMPLE.len() as i64);
    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
    assert!(!paths.temp.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_unknown_size_download_writes_final_file() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("unknown-complete");

    let downloaded = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/unknown-size", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: 0,
                supports_resume: false,

                supports_parallel: false,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("download");

    assert_eq!(downloaded, SAMPLE.len() as i64);
    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_conflicts_when_final_path_exists() {
    // ARC-02: finalize must not clobber or silently rename over an existing final.
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("final-conflict");
    let existing = b"existing user file";
    fs::write(&paths.final_path, existing).expect("seed existing final");

    let error = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/file", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("final path conflict");

    assert!(
        error.contains("final_path_conflict") || error.to_lowercase().contains("conflict"),
        "expected final_path_conflict, got: {error}"
    );
    assert_eq!(
        fs::read(&paths.final_path).expect("read existing"),
        existing
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_can_resume_from_temp_file() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("resume");
    let cancel = tokio_util::sync::CancellationToken::new();

    let first_cancel = cancel.clone();
    let first = tokio::spawn({
        let engine = engine.clone();
        let request = DirectDownloadRequest {
            url: format!("{}/slow-resume", server.base_url),
            temp_path: paths.temp.clone(),
            final_path: paths.final_path.clone(),
            total_size: slow_resume_payload().len() as i64,
            supports_resume: true,

            supports_parallel: true,
            etag: None,
            last_modified: None,
        };
        async move { engine.download_direct(request, first_cancel).await }
    });

    // A fixed 300 ms window could elapse before connect + first chunk under
    // parallel suite load (partial == 0); 2 s against the ~5.1 s /slow-resume
    // transfer keeps both assertions satisfied with wide margins.
    tokio::time::sleep(Duration::from_millis(2000)).await;
    cancel.cancel();
    let partial = first.await.expect("join").expect("partial");
    assert!(partial > 0);
    assert!(partial < slow_resume_payload().len() as i64);
    assert!(paths.temp.exists());

    engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/slow-resume", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: slow_resume_payload().len() as i64,
                supports_resume: true,

                supports_parallel: true,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("resume");

    assert_eq!(
        fs::read(&paths.final_path).expect("read final"),
        slow_resume_payload()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_respects_speed_limiter() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("speed-limit");
    let started = std::time::Instant::now();

    engine
        .download_direct_with_limiter(
            DirectDownloadRequest {
                url: format!("{}/slow", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: slow_payload().len() as i64,
                supports_resume: true,

                supports_parallel: true,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
            Arc::new(GlobalSpeedLimiter::new(Some(32 * 1024))),
        )
        .await
        .expect("limited download");

    assert!(started.elapsed() >= Duration::from_secs(1));
    assert_eq!(
        fs::read(&paths.final_path).expect("read final"),
        slow_payload()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_resume_fails_when_range_is_unavailable() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("no-range");
    fs::write(&paths.temp, b"partial").expect("write temp");

    let error = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/no-range", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_resume: false,

                supports_parallel: false,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("resume should fail");

    assert_eq!(
        error,
        "Resume unavailable. Restart this download from the beginning."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_resume_sends_if_range_and_validates_content_range() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("direct-if-range");
    fs::write(&paths.temp, &SAMPLE[..5]).expect("write temp");

    engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/requires-if-range", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: SAMPLE.len() as i64,
                supports_resume: true,
                supports_parallel: true,
                etag: Some("\"strong\"".to_string()),
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("resume with If-Range");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), SAMPLE);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_resume_fails_on_mismatched_content_range() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("direct-bad-content-range");
    let payload = large_payload();
    fs::write(&paths.temp, &payload[..1024]).expect("write temp");

    let error = engine
        .download_direct(
            DirectDownloadRequest {
                url: format!("{}/bad-content-range", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,
                supports_parallel: true,
                etag: None,
                last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("mismatched direct Content-Range should fail");

    assert_eq!(
        error,
        "Resume unavailable. The server returned a mismatched Content-Range."
    );
    assert!(!paths.final_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_download_writes_all_ranges_to_one_file() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-complete");
    let payload = large_payload();
    let segments = direct_segments("segmented-complete", payload.len() as i64);

    let downloaded = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/large", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("segmented download");

    assert_eq!(downloaded, payload.len() as i64);
    let final_bytes = fs::read(&paths.final_path).expect("read final");
    assert_eq!(sha256_hex(&final_bytes), LARGE_PAYLOAD_SHA256);
    assert_eq!(final_bytes, payload);
    assert!(!paths.temp.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_retries_transient_segment_failures() {
    std::env::set_var("VIBE_FAST_RETRY_DELAYS", "1");
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-retry");
    let payload = large_payload();

    engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/transient-segment", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments: direct_segments("segmented-retry", payload.len() as i64),
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("segmented retry");

    let final_bytes = fs::read(&paths.final_path).expect("read final");
    assert_eq!(sha256_hex(&final_bytes), LARGE_PAYLOAD_SHA256);
    std::env::remove_var("VIBE_FAST_RETRY_DELAYS");
}

/// ARC-56: `retry_count` persisted on a segment is a cumulative diagnostic
/// counter, not the retry budget. A segment that exhausted its budget during
/// an earlier outage (retry_count == MAX_SEGMENT_RETRIES) must still get a
/// full per-run allowance after the user hits "Retry" — before the fix the
/// worker started from the persisted counter and failed the task on the very
/// first transient error.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_retry_budget_resets_despite_persisted_retry_count() {
    std::env::set_var("VIBE_FAST_RETRY_DELAYS", "1");
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-retry-budget-reset");
    let payload = large_payload();
    // Simulate the DB state after a failed run: every segment carries an
    // exhausted cumulative counter (the coordinator would have persisted 5).
    let segments = direct_segments("segmented-retry-budget-reset", payload.len() as i64)
        .into_iter()
        .map(|mut segment| {
            segment.retry_count = 5;
            segment
        })
        .collect();

    // `/transient-segment` injects exactly one 500 per range start, so success
    // requires the worker to retry despite the persisted counter being at the
    // ceiling.
    let downloaded = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/transient-segment", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,
                supports_parallel: true,
                segments,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("retry budget must reset per run even when the persisted counter is exhausted");

    assert_eq!(downloaded, payload.len() as i64);
    let final_bytes = fs::read(&paths.final_path).expect("read final");
    assert_eq!(sha256_hex(&final_bytes), LARGE_PAYLOAD_SHA256);
    std::env::remove_var("VIBE_FAST_RETRY_DELAYS");
}

/// ARC-33: a connection abort mid-body must not let the checkpoint run ahead
/// of durable bytes. The abort leaves buffered-but-unflushed bytes in the
/// worker's 256 KiB BufWriter; if the retryable failure's offset were reported
/// without flushing, the resumed download would seek past a zero hole the
/// preallocated file never refills. The assembled file must match the payload
/// byte-for-byte.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_resume_after_mid_body_abort_writes_no_hole() {
    std::env::set_var("VIBE_FAST_RETRY_DELAYS", "1");
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-abort-hole");
    let payload = mid_abort_payload();

    let downloaded = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/abort-mid-body", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments: direct_segments("segmented-abort-hole", payload.len() as i64),
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("segmented download must recover from a mid-body connection abort");

    assert_eq!(downloaded, payload.len() as i64);
    let final_bytes = fs::read(&paths.final_path).expect("read final");
    assert_eq!(final_bytes, payload, "no zero hole may survive the abort");
    std::env::remove_var("VIBE_FAST_RETRY_DELAYS");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_resume_skips_completed_ranges() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-resume");
    let payload = large_payload();
    let mut segments = direct_segments("segmented-resume", payload.len() as i64);
    let first_end = segments[0].range_end as usize;
    fs::write(&paths.temp, &payload[..=first_end]).expect("write completed range");
    segments[0].downloaded_until = segments[0].range_end + 1;
    segments[0].status = SegmentStatus::Completed;

    engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/large", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments,
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect("resume segmented download");

    assert_eq!(fs::read(&paths.final_path).expect("read final"), payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_failure_does_not_rename_temp_file() {
    std::env::set_var("VIBE_FAST_RETRY_DELAYS", "1");
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-failure");
    let payload = large_payload();

    let error = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/segment-error", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments: direct_segments("segmented-failure", payload.len() as i64),
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("segment should fail");

    let payload: serde_json::Value = serde_json::from_str(&error).expect("500 error payload");
    assert_eq!(payload["code"], "server_error");
    assert_eq!(payload["message"], "The server returned HTTP 500.");
    assert!(payload.get("retryAfterAt").is_none());
    assert!(!paths.final_path.exists());
    std::env::remove_var("VIBE_FAST_RETRY_DELAYS");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_fails_on_mismatched_content_range() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-bad-content-range");
    let payload = large_payload();

    let error = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/bad-content-range", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments: direct_segments("segmented-bad-content-range", payload.len() as i64),
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("mismatched Content-Range should fail");

    assert_eq!(
        error,
        "Resume unavailable. The server returned a mismatched Content-Range."
    );
    assert!(!paths.final_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn segmented_direct_fails_when_range_is_not_honored() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-range-ignored");
    let payload = large_payload();

    let error = engine
        .download_segmented_direct(
            DirectSegmentedDownloadRequest {
                url: format!("{}/range-ignored", server.base_url),
                temp_path: paths.temp.clone(),
                final_path: paths.final_path.clone(),
                total_size: payload.len() as i64,
                supports_resume: true,

                supports_parallel: true,
                segments: direct_segments("segmented-range-ignored", payload.len() as i64),
                etag: None,
                last_modified: None,
            },
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .expect_err("ignored Range should fail");

    assert_eq!(
        error,
        "Resume unavailable. The server did not honor the byte range request."
    );
    assert!(!paths.final_path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_refuses_redirect_to_private_target() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/redirect-to-file", server.base_url);
    // The fake server itself is a task-authorized loopback target. The
    // redirect points at a different private authority, which must remain
    // outside that task grant even though the test harness permits loopback
    // connections for ordinary fixture traffic.
    let policy = tauri_app_lib::download::network_policy::NetworkPolicy::confirm_target(
        tauri_app_lib::download::network_policy::TaskSource::Manual,
        &url,
    )
    .await
    .expect("authorize fake server target");

    let error = engine
        .probe_with_headers_and_proxy_and_policy(&url, &[], None, &policy)
        .await
        .expect_err("redirects to loopback must not be followed");

    let payload: tauri_app_lib::models::AppErrorPayload =
        serde_json::from_str(&error).expect("redirect policy error must be structured");
    assert_eq!(payload.code, "intranet_target_blocked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_prefers_rfc5987_filename_over_plain() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/rfc5987-both", server.base_url))
        .await
        .expect("probe rfc5987");

    assert_eq!(probe.file_name, "encoded name.txt");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_ensures_extension_from_content_type() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let probe = engine
        .probe(&format!("{}/no-ext", server.base_url))
        .await
        .expect("probe no-ext");

    assert!(
        probe.file_name.ends_with(".pdf"),
        "expected .pdf extension from Content-Type, got: {}",
        probe.file_name
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn probe_maps_401_as_denied() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");

    let error = engine
        .probe(&format!("{}/status/401", server.base_url))
        .await
        .expect_err("401 should fail");

    let value: serde_json::Value = serde_json::from_str(&error).expect("401 payload");
    assert_eq!(value["code"], "http_denied");
    assert_eq!(value["message"], "The server denied access to this file.");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_download_cancel_mid_stream_returns_partial() {
    let payload = slow_payload();
    let served = Arc::new(AtomicUsize::new(0));
    let server = start_counting_slow_server(payload.clone(), served.clone());
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("cancel-mid");
    let cancel = tokio_util::sync::CancellationToken::new();

    let cancel_clone = cancel.clone();
    let engine_clone = engine.clone();
    let request = DirectDownloadRequest {
        url: format!("{}/slow", server.base_url),
        temp_path: paths.temp.clone(),
        final_path: paths.final_path.clone(),
        total_size: payload.len() as i64,
        supports_resume: true,
        supports_parallel: true,
        etag: None,
        last_modified: None,
    };

    let handle =
        tokio::spawn(async move { engine_clone.download_direct(request, cancel_clone).await });

    // Cancel on a real progress signal, not a timer or the temp file. A fixed
    // sleep races task startup under full-suite load (cancel can fire before the
    // first chunk arrives -> `downloaded == 0`), and the temp file never grows
    // mid-stream because `download_direct` buffers through a 256 KiB BufWriter
    // that is flushed only on cancel/completion. The server's served-bytes counter
    // is the one thing guaranteed to move while the stream is still in flight, so
    // waiting on it proves bytes were pushed before we stop the download.
    for _ in 0..300 {
        if served.load(Ordering::SeqCst) >= 8 * 1024 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cancel.cancel();

    let result = handle.await.expect("join");
    let downloaded = result.expect("cancel should return Ok with partial bytes");
    assert!(
        downloaded > 0,
        "should have downloaded some bytes before cancel"
    );
    assert!(
        downloaded < payload.len() as i64,
        "should not have completed the full download"
    );
    // Temp file should still exist (not finalized)
    assert!(paths.temp.exists(), "temp file should remain after cancel");
    assert!(
        !paths.final_path.exists(),
        "final file should not exist after cancel"
    );
}

/// Single-route server that streams `payload` for any GET in paced 1 KiB chunks,
/// recording how many bytes it has pushed into the socket via `served`. This gives
/// the cancel test an observable in-flight progress signal that neither a fixed
/// sleep nor the (buffered, never mid-stream-flushed) temp file can provide.
fn start_counting_slow_server(payload: Vec<u8>, served: Arc<AtomicUsize>) -> TestServer {
    TestServer::start(move |mut stream| {
        let mut buffer = [0_u8; 4096];
        // The Drop guard opens a dummy connection to wake the accept loop; it sends
        // no request, so bail before streaming anything for it.
        let Ok(read) = stream.read(&mut buffer) else {
            return;
        };
        if read == 0 {
            return;
        }
        let total = payload.len();
        let head = format!(
            "HTTP/1.1 200 OK\r\n\
             Connection: close\r\n\
             Content-Length: {total}\r\n\
             Content-Type: application/octet-stream\r\n\
             Accept-Ranges: bytes\r\n\
             Content-Disposition: attachment; filename=\"slow.bin\"\r\n\r\n"
        );
        let _ = stream.write_all(head.as_bytes());
        for chunk in payload.chunks(1024) {
            let _ = stream.write_all(chunk);
            let _ = stream.flush();
            served.fetch_add(chunk.len(), Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
        }
    })
}

fn start_test_server() -> TestServer {
    let state: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));
    TestServer::start(move |stream| handle_connection(stream, state.clone()))
}

fn handle_connection(mut stream: TcpStream, state: Arc<Mutex<HashMap<String, usize>>>) {
    let mut buffer = [0_u8; 4096];
    let Ok(read) = stream.read(&mut buffer) else {
        return;
    };
    if read == 0 {
        return;
    }

    let request = String::from_utf8_lossy(&buffer[..read]);
    let mut lines = request.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/");
    let byte_range = request.lines().find_map(parse_range);
    let if_range = request
        .lines()
        .find_map(|line| parse_header(line, "if-range"));
    let accept_encoding_identity = request.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("accept-encoding")
                && value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("identity"))
        })
    });

    match path {
        "/requires-identity" if !accept_encoding_identity => {
            write_response(&mut stream, 400, &[], b"identity required", false)
        }
        "/requires-identity" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "identity.bin",
            false,
        ),
        "/file" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "sample.bin",
            false,
        ),
        "/basic-auth" => {
            // FUN-01: require Authorization: Basic dXNlcjpwYXNz (user:pass).
            let authorized = request.lines().any(|line| {
                line.split_once(':').is_some_and(|(name, value)| {
                    name.eq_ignore_ascii_case("authorization")
                        && value.trim() == "Basic dXNlcjpwYXNz"
                })
            });
            if !authorized {
                write_response(
                    &mut stream,
                    401,
                    &[("WWW-Authenticate", "Basic realm=\"vibe\"")],
                    b"unauthorized",
                    false,
                );
            } else {
                respond_file(
                    &mut stream,
                    method,
                    SAMPLE,
                    byte_range,
                    true,
                    "protected.bin",
                    false,
                );
            }
        }
        "/requires-if-range"
            if byte_range.is_some() && if_range.as_deref() != Some("\"strong\"") =>
        {
            write_response(&mut stream, 412, &[], b"if-range required", false)
        }
        "/requires-if-range" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "if-range.bin",
            false,
        ),
        "/head-no-length" if method == "HEAD" => {
            write_response(&mut stream, 200, &[("Accept-Ranges", "bytes")], &[], false)
        }
        "/head-no-length" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            byte_range,
            true,
            "fallback.bin",
            false,
        ),
        "/unknown-size" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[("Content-Type", "application/octet-stream")],
        ),
        "/unknown-malformed-chunk" => {
            let etag = ("ETag", "\"malformed\"");
            if method == "HEAD" {
                write_unknown_size_response(
                    &mut stream,
                    method,
                    SAMPLE,
                    &[("Content-Type", "application/octet-stream"), etag],
                );
            } else if byte_range.is_some() {
                write_response(
                    &mut stream,
                    206,
                    &[
                        ("Content-Type", "application/octet-stream"),
                        etag,
                        ("Content-Range", "bytes 0-0/*"),
                        ("Content-Length", "1"),
                    ],
                    &SAMPLE[..1],
                    false,
                );
            } else {
                let response = "HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nETag: \"malformed\"\r\n\r\nZ\r\n";
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        }
        "/unknown-range-stable" => respond_unknown_size_range(
            &mut stream,
            method,
            byte_range,
            Some("\"stable\""),
            false,
            false,
        ),
        "/unknown-range-invalid-probe" => respond_unknown_size_range(
            &mut stream,
            method,
            byte_range,
            Some("\"stable\""),
            true,
            false,
        ),
        "/unknown-range-weak-validator" => respond_unknown_size_range(
            &mut stream,
            method,
            byte_range,
            Some("W/\"weak\""),
            false,
            false,
        ),
        "/unknown-range-changed-validator" => respond_unknown_size_range(
            &mut stream,
            method,
            byte_range,
            Some("\"changed\""),
            false,
            false,
        ),
        "/unknown-range-missing-validator" => {
            respond_unknown_size_range(&mut stream, method, byte_range, None, false, false)
        }
        "/unknown-range-invalid-resume" => respond_unknown_size_range(
            &mut stream,
            method,
            byte_range,
            Some("\"stable\""),
            false,
            true,
        ),
        "/unknown-resume-after-reset" => {
            handle_unknown_resume_connection(stream, method, byte_range, if_range, state)
        }
        "/unknown-restart-after-reset" => {
            handle_unknown_restart_connection(stream, method, byte_range, state)
        }
        "/content-location-name" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[
                ("Content-Type", "application/pdf"),
                ("Content-Location", "/exports/report"),
            ],
        ),
        target if target.starts_with("/query-name") => {
            respond_file_without_disposition(&mut stream, method, SAMPLE, byte_range, true, false)
        }
        "/encoded-name" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[
                ("Content-Type", "application/octet-stream"),
                (
                    "Content-Disposition",
                    "attachment; filename*=UTF-8''encoded%20name.txt",
                ),
            ],
        ),
        "/transient-segment" if byte_range.is_some_and(|range| range.start > 0) => {
            let key = format!(
                "transient-{}",
                byte_range.map(|range| range.start).unwrap_or(0)
            );
            let mut state = state.lock().expect("state lock");
            let count = state.entry(key).or_insert(0);
            if *count == 0 {
                *count += 1;
                write_response(&mut stream, 500, &[], b"retry later", false);
            } else {
                drop(state);
                respond_file(
                    &mut stream,
                    method,
                    &large_payload(),
                    byte_range,
                    true,
                    "transient.bin",
                    false,
                );
            }
        }
        "/transient-segment" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            byte_range,
            true,
            "transient.bin",
            false,
        ),
        // ARC-33: retry attempts (range start > 0) see a well-formed 206 with
        // the full range; only the first attempt (range start == 0) is aborted
        // mid-body below.
        "/abort-mid-body" if byte_range.is_some_and(|range| range.start > 0) => respond_file(
            &mut stream,
            method,
            &mid_abort_payload(),
            byte_range,
            true,
            "abort-mid-body.bin",
            false,
        ),
        "/abort-mid-body" if byte_range.is_some() => {
            let range = byte_range.expect("guarded by the match arm");
            let payload = mid_abort_payload();
            let start = range.start;
            let end = range.end.unwrap_or_else(|| payload.len().saturating_sub(1));
            let available = end - start + 1;
            // A contract-valid 206 head: the worker's Content-Range check must
            // pass so the abort lands mid-body (a retryable connection error),
            // not at the response validation (non-retryable).
            let head = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\n\
                 Content-Range: bytes {start}-{end}/{}\r\n\
                 Content-Length: {available}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            let _ = stream.write_all(head.as_bytes());
            // Stop strictly past one 256 KiB BufWriter flush boundary but
            // before the first segment's end, so the worker holds a non-empty
            // unflushed residue when the connection dies.
            let serve = MID_ABORT_OFFSET.min(available);
            let _ = stream.write_all(&payload[start..start + serve]);
            let _ = stream.flush();
            // SO_LINGER=0 turns close into a TCP RST so the client's next
            // chunk() surfaces as a connection error, not a clean EOF.
            // std's set_linger is nightly-only, hence socket2 here.
            let raw: socket2::Socket = socket2::Socket::from(stream);
            let _ = raw.set_linger(Some(Duration::ZERO));
            let _ = raw.shutdown(std::net::Shutdown::Both);
        }
        "/slow" => respond_file(
            &mut stream,
            method,
            &slow_payload(),
            byte_range,
            true,
            "slow.bin",
            true,
        ),
        // 512 KiB at 10 ms/1 KiB-chunk ≈ 5.1 s of transfer: wide margins on
        // both sides of the resume test's 2 s cancel window even under heavy
        // parallel suite load (the 64 KiB /slow transfer finishes in ~0.65 s,
        // so its usable cancel window is too narrow to be load-tolerant).
        // ARC-27: a long server-imposed backoff that the test cancels out of.
        "/retry-after-forever" => {
            let response = "HTTP/1.1 429 Too Many Requests
Retry-After: 60
Content-Length: 0
Connection: close

";
            let _ = stream.write_all(response.as_bytes());
        }
        "/slow-resume" => respond_file(
            &mut stream,
            method,
            &slow_resume_payload(),
            byte_range,
            true,
            "slow-resume.bin",
            true,
        ),
        "/large" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            byte_range,
            true,
            "large.bin",
            false,
        ),
        "/bad-content-range" => {
            respond_bad_content_range(&mut stream, method, &large_payload(), byte_range)
        }
        "/range-ignored" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            None,
            false,
            "range-ignored.bin",
            false,
        ),
        "/segment-error" if byte_range.is_some_and(|range| range.start > 0) => {
            write_response(&mut stream, 500, &[], b"segment failed", false)
        }
        "/segment-error" => respond_file(
            &mut stream,
            method,
            &large_payload(),
            byte_range,
            true,
            "segment-error.bin",
            false,
        ),
        "/no-range" => respond_file(
            &mut stream,
            method,
            SAMPLE,
            None,
            false,
            "no-range.bin",
            false,
        ),
        "/redirect-to-file" => {
            let host = request
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("host").then_some(value.trim())
                })
                .unwrap_or_default();
            let port = host.split(':').nth(1).unwrap_or("80");
            let location = format!("http://127.0.0.2:{port}/file");
            let response = format!(
                "HTTP/1.1 302 Found\r\nConnection: close\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        }
        "/rfc5987-both" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[
                ("Content-Type", "application/octet-stream"),
                (
                    "Content-Disposition",
                    "attachment; filename=\"plain.txt\"; filename*=UTF-8''encoded%20name.txt",
                ),
            ],
        ),
        "/no-ext" => write_unknown_size_response(
            &mut stream,
            method,
            SAMPLE,
            &[("Content-Type", "application/pdf")],
        ),
        "/status/401" => write_response(&mut stream, 401, &[], b"unauthorized", false),
        "/status/403" => write_response(&mut stream, 403, &[], b"denied", false),
        "/status/404" => write_response(&mut stream, 404, &[], b"missing", false),
        "/status/429" => write_response(&mut stream, 429, &[], b"limited", false),
        "/status/429-retry-seconds" => write_response(
            &mut stream,
            429,
            &[("Retry-After", "45")],
            b"limited",
            false,
        ),
        "/status/429-retry-date" => write_response(
            &mut stream,
            429,
            &[("Retry-After", "Wed, 02 Jan 2030 03:04:05 GMT")],
            b"limited",
            false,
        ),
        _ => write_response(&mut stream, 404, &[], b"missing", false),
    }
}

fn respond_file(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
    supports_parallel: bool,
    file_name: &str,
    slow: bool,
) {
    let start = byte_range
        .map(|range| range.start)
        .unwrap_or(0)
        .min(payload.len());
    let end = byte_range
        .and_then(|range| range.end)
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let status = if byte_range.is_some() && supports_parallel {
        206
    } else {
        200
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {start}-{end}/{}", payload.len());
    let disposition = format!("attachment; filename=\"{file_name}\"");
    let mut headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
        ("Content-Disposition", disposition.as_str()),
    ];
    if supports_parallel {
        headers.push(("Accept-Ranges", "bytes"));
    }
    if status == 206 {
        headers.push(("Content-Range", content_range.as_str()));
    }

    write_response(stream, status, &headers, body, slow);
}

fn respond_bad_content_range(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
) {
    let Some(range) = byte_range else {
        respond_file(stream, method, payload, None, true, "bad-range.bin", false);
        return;
    };
    let start = range.start.min(payload.len());
    let end = range
        .end
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {}-{end}/{}", start.saturating_add(1), payload.len());
    let disposition = "attachment; filename=\"bad-range.bin\"";
    let headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
        ("Content-Disposition", disposition),
        ("Accept-Ranges", "bytes"),
        ("Content-Range", content_range.as_str()),
    ];
    write_response(stream, 206, &headers, body, false);
}

fn respond_file_without_disposition(
    stream: &mut TcpStream,
    method: &str,
    payload: &[u8],
    byte_range: Option<ByteRange>,
    supports_parallel: bool,
    slow: bool,
) {
    let start = byte_range
        .map(|range| range.start)
        .unwrap_or(0)
        .min(payload.len());
    let end = byte_range
        .and_then(|range| range.end)
        .unwrap_or_else(|| payload.len().saturating_sub(1))
        .min(payload.len().saturating_sub(1));
    let body = if method == "HEAD" || start > end {
        &[][..]
    } else {
        &payload[start..=end]
    };
    let status = if byte_range.is_some() && supports_parallel {
        206
    } else {
        200
    };
    let content_length = if method == "HEAD" {
        payload.len().to_string()
    } else {
        body.len().to_string()
    };
    let content_range = format!("bytes {start}-{end}/{}", payload.len());
    let mut headers = vec![
        ("Content-Length", content_length.as_str()),
        ("Content-Type", "application/octet-stream"),
    ];
    if supports_parallel {
        headers.push(("Accept-Ranges", "bytes"));
    }
    if status == 206 {
        headers.push(("Content-Range", content_range.as_str()));
    }

    write_response(stream, status, &headers, body, slow);
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
    slow: bool,
) {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let mut response = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if slow {
        for chunk in body.chunks(1024) {
            let _ = stream.write_all(chunk);
            let _ = stream.flush();
            thread::sleep(Duration::from_millis(10));
        }
    } else {
        let _ = stream.write_all(body);
        let _ = stream.flush();
    }
}

fn write_unknown_size_response(
    stream: &mut TcpStream,
    method: &str,
    body: &[u8],
    headers: &[(&str, &str)],
) {
    let mut response = "HTTP/1.1 200 OK\r\nConnection: close\r\n".to_string();
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(body);
    }
    let _ = stream.flush();
}

fn respond_unknown_size_range(
    stream: &mut TcpStream,
    method: &str,
    byte_range: Option<ByteRange>,
    etag: Option<&str>,
    invalid_probe: bool,
    invalid_resume: bool,
) {
    let mut headers = vec![("Content-Type", "application/octet-stream")];
    if let Some(etag) = etag {
        headers.push(("ETag", etag));
    }
    if method == "HEAD" {
        write_unknown_size_response(stream, method, SAMPLE, &headers);
        return;
    }

    let Some(range) = byte_range else {
        write_unknown_size_response(stream, method, SAMPLE, &headers);
        return;
    };
    let mut start = range.start.min(SAMPLE.len().saturating_sub(1));
    let mut end = range
        .end
        .unwrap_or_else(|| SAMPLE.len().saturating_sub(1))
        .min(SAMPLE.len().saturating_sub(1));
    if invalid_probe && start == 0 {
        end = end.saturating_add(1).min(SAMPLE.len().saturating_sub(1));
    }
    if invalid_resume && start > 0 {
        start = start.saturating_add(1);
    }
    if start > end {
        write_response(stream, 416, &[], b"range not satisfiable", false);
        return;
    }

    let content_range = format!("bytes {start}-{end}/*");
    let content_length = (end - start + 1).to_string();
    headers.push(("Content-Range", content_range.as_str()));
    headers.push(("Content-Length", content_length.as_str()));
    write_response(stream, 206, &headers, &SAMPLE[start..=end], false);
}

fn handle_unknown_resume_connection(
    mut stream: TcpStream,
    method: &str,
    byte_range: Option<ByteRange>,
    if_range: Option<String>,
    state: Arc<Mutex<HashMap<String, usize>>>,
) {
    const ETAG: &str = "\"stable\"";
    let payload = slow_resume_payload();
    let headers = [("Content-Type", "application/octet-stream"), ("ETag", ETAG)];

    if method == "HEAD" {
        write_unknown_size_response(&mut stream, method, &payload, &headers);
        return;
    }

    if let Some(range) = byte_range {
        if range.start > 0 && if_range.as_deref() != Some(ETAG) {
            write_unknown_size_response(&mut stream, method, &payload, &headers);
            return;
        }
        let start = range.start.min(payload.len().saturating_sub(1));
        let end = range
            .end
            .unwrap_or_else(|| payload.len().saturating_sub(1))
            .min(payload.len().saturating_sub(1));
        if start > end {
            write_response(&mut stream, 416, &[], b"range not satisfiable", false);
            return;
        }
        let content_range = format!("bytes {start}-{end}/*");
        let content_length = (end - start + 1).to_string();
        let mut range_headers = headers.to_vec();
        range_headers.push(("Content-Range", content_range.as_str()));
        range_headers.push(("Content-Length", content_length.as_str()));
        write_response(
            &mut stream,
            206,
            &range_headers,
            &payload[start..=end],
            false,
        );
        return;
    }

    let first_transfer = {
        let mut state = state.lock().expect("state lock");
        let count = state
            .entry("unknown-resume-after-reset".to_string())
            .or_insert(0);
        let first = *count == 0;
        *count += 1;
        first
    };
    if !first_transfer {
        write_unknown_size_response(&mut stream, method, &payload, &headers);
        return;
    }

    let response_head = format!(
        "HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\nETag: {ETAG}\r\n\r\n"
    );
    let chunk_head = format!("{:X}\r\n", 300_000);
    let _ = stream.write_all(response_head.as_bytes());
    let _ = stream.write_all(chunk_head.as_bytes());
    let _ = stream.write_all(&payload[..300_000]);
    let _ = stream.write_all(b"\r\n");
    let _ = stream.flush();
    let socket: socket2::Socket = stream.into();
    let _ = socket.set_linger(Some(Duration::ZERO));
    let _ = socket.shutdown(std::net::Shutdown::Both);
}

fn handle_unknown_restart_connection(
    mut stream: TcpStream,
    method: &str,
    byte_range: Option<ByteRange>,
    state: Arc<Mutex<HashMap<String, usize>>>,
) {
    let payload = slow_resume_payload();
    if method == "HEAD" || byte_range.is_some() {
        write_unknown_size_response(&mut stream, method, &payload, &[]);
        return;
    }

    let first_transfer = {
        let mut state = state.lock().expect("state lock");
        let count = state
            .entry("unknown-restart-after-reset".to_string())
            .or_insert(0);
        let first = *count == 0;
        *count += 1;
        first
    };
    if !first_transfer {
        write_unknown_size_response(&mut stream, method, &payload, &[]);
        return;
    }

    let response_head = "HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\n\r\n";
    let chunk_head = format!("{:X}\r\n", 300_000);
    let _ = stream.write_all(response_head.as_bytes());
    let _ = stream.write_all(chunk_head.as_bytes());
    let _ = stream.write_all(&payload[..300_000]);
    let _ = stream.write_all(b"\r\n");
    let _ = stream.flush();
    let socket: socket2::Socket = stream.into();
    let _ = socket.set_linger(Some(Duration::ZERO));
    let _ = socket.shutdown(std::net::Shutdown::Both);
}

fn slow_payload() -> Vec<u8> {
    (0..65_536).map(|index| (index % 251) as u8).collect()
}

fn slow_resume_payload() -> Vec<u8> {
    (0..(512 * 1024)).map(|index| (index % 251) as u8).collect()
}

fn large_payload() -> Vec<u8> {
    (0..(16 * 1024 * 1024 + 13))
        .map(|index| (index % 251) as u8)
        .collect()
}

/// Payload for the mid-body abort route: four 300 KB segments, each larger
/// than the worker's 256 KiB BufWriter so an abort strictly inside a segment
/// leaves buffered-but-unflushed bytes behind.
fn mid_abort_payload() -> Vec<u8> {
    (0..(4 * 300_000))
        .map(|index| (index % 251) as u8)
        .collect()
}

const MID_ABORT_OFFSET: usize = 256 * 1024 + 10_000;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn direct_segments(task_id: &str, total_size: i64) -> Vec<TaskSegmentRecord> {
    let count = 4_i64;
    let base = total_size / count;
    let remainder = total_size % count;
    let mut start = 0_i64;

    (0..count)
        .map(|index| {
            let length = base + if index < remainder { 1 } else { 0 };
            let end = start + length - 1;
            let segment = TaskSegmentRecord {
                id: format!("{task_id}-segment-{index}"),
                task_id: task_id.to_string(),
                file_id: None,
                unit_kind: "http_range".to_string(),
                range_start: start,
                range_end: end,
                downloaded_until: start,
                speed_bps: 0,
                status: SegmentStatus::Pending,
                retry_count: 0,
                last_error: None,
            };
            start = end + 1;
            segment
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ByteRange {
    start: usize,
    end: Option<usize>,
}

fn parse_range(line: &str) -> Option<ByteRange> {
    let (name, value) = line.split_once(':')?;
    if !name.eq_ignore_ascii_case("range") {
        return None;
    }
    let (start, end) = value.trim().strip_prefix("bytes=")?.split_once('-')?;
    Some(ByteRange {
        start: start.parse::<usize>().ok()?,
        end: if end.is_empty() {
            None
        } else {
            Some(end.parse::<usize>().ok()?)
        },
    })
}

fn parse_header(line: &str, expected_name: &str) -> Option<String> {
    let (name, value) = line.split_once(':')?;
    name.eq_ignore_ascii_case(expected_name)
        .then(|| value.trim().to_string())
}

/// ARC-27: a worker sleeping in a server-imposed Retry-After backoff must
/// converge as soon as the user cancels — not after the full 60s backoff.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn segmented_direct_cancel_during_retry_backoff_converges_quickly() {
    let server = start_test_server();
    let engine = HttpEngine::new().expect("engine");
    let paths = TestPaths::new("segmented-backoff-cancel");
    let payload = mid_abort_payload();
    let cancel = tokio_util::sync::CancellationToken::new();

    let started = std::time::Instant::now();
    let download = {
        let engine = engine.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            engine
                .download_segmented_direct(
                    DirectSegmentedDownloadRequest {
                        url: format!("{}/retry-after-forever", server.base_url),
                        temp_path: paths.temp.clone(),
                        final_path: paths.final_path.clone(),
                        total_size: payload.len() as i64,
                        supports_resume: true,
                        supports_parallel: true,
                        segments: direct_segments("segmented-backoff-cancel", payload.len() as i64),
                        etag: None,
                        last_modified: None,
                    },
                    cancel,
                )
                .await
        })
    };

    // Let every worker enter its 60s backoff, then cancel.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    cancel.cancel();

    let result = tokio::time::timeout(std::time::Duration::from_secs(10), download).await;
    assert!(
        result.is_ok(),
        "ARC-27 regression: cancel during Retry-After backoff did not converge quickly"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(9),
        "convergence took {:?}; the worker slept through the backoff",
        started.elapsed()
    );
}
