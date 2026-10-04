//! FUN-39: persistence, expiry and actual HTTP origin boundaries.

mod common;

use sqlx::Row;
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
};
use tauri_app_lib::{
    db,
    download::HttpEngine,
    models::{
        normalize_task_request_profile, AppErrorPayload, SegmentStatus, TaskRequestHeaderInput,
        TaskRequestProfileInput, TaskSegmentRecord,
    },
};

fn profile() -> TaskRequestProfileInput {
    TaskRequestProfileInput {
        user_agent: Some("ProfileRegression/1".into()),
        referer: Some("https://example.com/page".into()),
        custom_headers: vec![
            TaskRequestHeaderInput {
                name: "Accept-Language".into(),
                value: "zh-CN".into(),
            },
            TaskRequestHeaderInput {
                name: "Cookie".into(),
                value: "session=profile-secret".into(),
            },
            TaskRequestHeaderInput {
                name: "X-Token".into(),
                value: "profile-api-secret".into(),
            },
        ],
    }
}

async fn persist(pool: &sqlx::SqlitePool, id: &str, url: &str, input: &TaskRequestProfileInput) {
    let prepared = db::prepare_task_request_profile(url, input).expect("prepare");
    let mut conn = pool.acquire().await.expect("conn");
    db::save_task_request_profile(&mut conn, id, &prepared)
        .await
        .expect("persist");
}

#[test]
fn profile_validation_rejects_engine_owned_headers_injection_duplicates_and_limits() {
    for name in [
        "Host",
        "Range",
        "If-Range",
        "Accept-Encoding",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Authorization",
        "Proxy-Authorization",
        "Set-Cookie",
        "Sec-Fetch-Site",
        "X-Forwarded-For",
        "X-Real-IP",
        "X-Proxy-Token",
        "User-Agent",
    ] {
        let input = TaskRequestProfileInput {
            custom_headers: vec![TaskRequestHeaderInput {
                name: name.into(),
                value: "secret".into(),
            }],
            ..Default::default()
        };
        let err =
            normalize_task_request_profile("https://example.com/file", &input).expect_err(name);
        let payload: AppErrorPayload = serde_json::from_str(&err).expect("structured");
        assert_eq!(payload.code, "request_profile_invalid");
        assert!(!err.contains("secret"));
    }
    for name in [
        "\nCookie".to_string(),
        "Cookie\t".to_string(),
        format!("{}Cookie", " ".repeat(129)),
    ] {
        let input = TaskRequestProfileInput {
            custom_headers: vec![TaskRequestHeaderInput {
                name,
                value: "secret".into(),
            }],
            ..Default::default()
        };
        assert!(normalize_task_request_profile("https://example.com/file", &input).is_err());
    }
    for value in ["agent\r\nCookie: secret", "agent\n", "agent\t", "agent\0"] {
        let input = TaskRequestProfileInput {
            user_agent: Some(value.into()),
            ..Default::default()
        };
        assert!(normalize_task_request_profile("https://example.com/file", &input).is_err());
    }
    for referer in [
        "file:///tmp/secret",
        "https://user:secret@example.com/",
        "https://example.com/#secret",
    ] {
        let input = TaskRequestProfileInput {
            referer: Some(referer.into()),
            ..Default::default()
        };
        assert!(normalize_task_request_profile("https://example.com/file", &input).is_err());
    }
    let mut duplicate = profile();
    duplicate.custom_headers.push(TaskRequestHeaderInput {
        name: "COOKIE".into(),
        value: "other".into(),
    });
    assert!(normalize_task_request_profile("https://example.com/file", &duplicate).is_err());
    let oversized = TaskRequestProfileInput {
        user_agent: Some("x".repeat(8193)),
        ..Default::default()
    };
    assert!(normalize_task_request_profile("https://example.com/file", &oversized).is_err());
    let many = TaskRequestProfileInput {
        custom_headers: (0..17)
            .map(|i| TaskRequestHeaderInput {
                name: format!("x-{i}"),
                value: "a".into(),
            })
            .collect(),
        ..Default::default()
    };
    assert!(normalize_task_request_profile("https://example.com/file", &many).is_err());
    let total = TaskRequestProfileInput {
        user_agent: Some("x".repeat(8192)),
        custom_headers: vec![TaskRequestHeaderInput {
            name: "X-Token".into(),
            value: "y".repeat(8192),
        }],
        ..Default::default()
    };
    assert!(normalize_task_request_profile("https://example.com/file", &total).is_err());
    for url in [
        "ftp://example.com/file",
        "sftp://example.com/file",
        "magnet:?xt=abc",
        "https://example.com/file.torrent",
    ] {
        let err = normalize_task_request_profile(url, &profile()).expect_err(url);
        assert_eq!(
            serde_json::from_str::<AppErrorPayload>(&err)
                .expect("structured")
                .code,
            "request_profile_unsupported"
        );
    }
}

#[tokio::test]
async fn profile_is_encrypted_and_public_edits_preserve_secret_expiry() {
    common::install_test_secret_key();
    let (_db, pool) = common::test_pool("request-profile-encryption").await;
    let paths = common::TestPaths::new("profile");
    let task = common::download_task(
        "profile",
        "https://example.com/file".into(),
        "https",
        "file.bin",
        10,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task).await.expect("task");
    persist(&pool, &task.id, &task.url, &profile()).await;
    let row = sqlx::query("SELECT * FROM task_request_profiles WHERE task_id=?")
        .bind(&task.id)
        .fetch_one(&pool)
        .await
        .expect("row");
    for column in ["public_ciphertext", "sensitive_ciphertext"] {
        let ciphertext: String = row.get(column);
        for raw in [
            "ProfileRegression",
            "example.com/page",
            "profile-secret",
            "profile-api-secret",
        ] {
            assert!(!ciphertext.contains(raw));
        }
    }
    let expiry: String = row.get("sensitive_expires_at");
    let public_edit = TaskRequestProfileInput {
        user_agent: Some("Updated/2".into()),
        ..Default::default()
    };
    db::update_task_request_profile(&pool, &task.id, &task.url, &public_edit, false)
        .await
        .expect("public edit");
    let view = db::get_task_request_profile(&pool, &task.id)
        .await
        .expect("view");
    assert_eq!(view.user_agent.as_deref(), Some("Updated/2"));
    assert_eq!(view.sensitive_expires_at.as_deref(), Some(expiry.as_str()));
    assert_eq!(view.sensitive_header_names, vec!["cookie", "x-token"]);
    let view_json = serde_json::to_string(&view).expect("view json");
    assert!(!view_json.contains("profile-secret") && !view_json.contains("profile-api-secret"));
    let headers = db::resolve_task_request_profile_headers(&pool, &task.id)
        .await
        .expect("resolve");
    assert!(headers
        .iter()
        .any(|(name, value)| name == "cookie" && value == "session=profile-secret"));
    db::validate_backup_secrets(&pool)
        .await
        .expect("valid encrypted backup");
    sqlx::query("UPDATE task_request_profiles SET sensitive_nonce='bad' WHERE task_id=?")
        .bind(&task.id)
        .execute(&pool)
        .await
        .expect("corrupt nonce");
    let error = db::resolve_task_request_profile_headers(&pool, &task.id)
        .await
        .expect_err("fail closed");
    assert_eq!(
        serde_json::from_str::<AppErrorPayload>(&error)
            .expect("payload")
            .code,
        "auth_headers_unavailable"
    );
    assert!(!error.contains("profile-secret"));
    assert!(db::validate_backup_secrets(&pool).await.is_err());
}

#[tokio::test]
async fn expiry_wipes_only_secrets_and_requires_refresh_on_every_retry() {
    common::install_test_secret_key();
    let (_db, pool) = common::test_pool("profile-expiry").await;
    let paths = common::TestPaths::new("expiry");
    let task = common::download_task(
        "expiry",
        "https://example.com/file".into(),
        "https",
        "file.bin",
        10,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task).await.expect("task");
    persist(&pool, &task.id, &task.url, &profile()).await;
    sqlx::query("UPDATE task_request_profiles SET sensitive_expires_at='2000-01-01T00:00:00Z' WHERE task_id=?")
        .bind(&task.id).execute(&pool).await.expect("expire");
    for _ in 0..2 {
        let err = db::resolve_task_request_profile_headers(&pool, &task.id)
            .await
            .expect_err("expired");
        assert_eq!(
            serde_json::from_str::<AppErrorPayload>(&err)
                .expect("payload")
                .code,
            "auth_headers_expired"
        );
    }
    let row = sqlx::query(
        "SELECT sensitive_ciphertext, sensitive_nonce FROM task_request_profiles WHERE task_id=?",
    )
    .bind(&task.id)
    .fetch_one(&pool)
    .await
    .expect("row");
    assert!(row
        .get::<Option<String>, _>("sensitive_ciphertext")
        .is_none());
    assert!(row.get::<Option<String>, _>("sensitive_nonce").is_none());
    let view = db::get_task_request_profile(&pool, &task.id)
        .await
        .expect("view");
    assert!(view.sensitive_expired);
    assert_eq!(view.user_agent.as_deref(), Some("ProfileRegression/1"));
    assert_eq!(view.referer.as_deref(), Some("https://example.com/page"));
    db::update_task_request_profile(&pool, &task.id, &task.url, &profile(), true)
        .await
        .expect("refresh");
    assert_eq!(
        db::resolve_task_request_profile_headers(&pool, &task.id)
            .await
            .expect("new headers")
            .len(),
        5
    );
    let public = TaskRequestProfileInput {
        user_agent: Some("Durable/1".into()),
        ..Default::default()
    };
    db::update_task_request_profile(&pool, &task.id, &task.url, &public, true)
        .await
        .expect("remove secrets");
    assert!(db::get_task_request_profile(&pool, &task.id)
        .await
        .expect("view")
        .sensitive_expires_at
        .is_none());
}

fn capture_file_server(
    payload: Arc<Vec<u8>>,
    captured: Arc<Mutex<Vec<String>>>,
) -> common::TestServer {
    common::TestServer::start(move |mut stream| {
        let mut raw = Vec::new();
        let mut buf = [0; 2048];
        while raw.len() < 32768 {
            let read = stream.read(&mut buf).unwrap_or(0);
            if read == 0 {
                return;
            }
            raw.extend_from_slice(&buf[..read]);
            if raw.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8(raw).expect("request text");
        let method = text.split_whitespace().next().unwrap_or("GET");
        let range = text.lines().find_map(common::http::parse_range);
        captured.lock().expect("capture").push(text.clone());
        common::http::respond_file(
            &mut stream,
            method,
            &payload,
            range,
            true,
            "file.bin",
            false,
        );
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persisted_profile_reaches_probe_and_resumed_parallel_download_after_reopen() {
    common::install_test_secret_key();
    let payload = Arc::new((0..256 * 1024).map(|i| (i % 251) as u8).collect::<Vec<_>>());
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = capture_file_server(payload.clone(), captured.clone());
    let engine = HttpEngine::new().expect("engine");
    let url = format!("{}/file", server.base_url);
    let normalized = normalize_task_request_profile(&url, &profile()).expect("normalize");
    engine
        .probe_with_headers(&url, &normalized)
        .await
        .expect("probe");
    let (_db, pool) = common::test_pool("profile-reopen").await;
    let paths = common::TestPaths::new("profile-resume");
    let mut task = common::download_task(
        "resumed",
        url,
        "http",
        "file.bin",
        payload.len() as i64,
        &paths,
        true,
    );
    task.downloaded_bytes = 8192;
    db::insert_task_record(&pool, &task).await.expect("task");
    persist(&pool, &task.id, &task.url, &profile()).await;
    for index in 0..2 {
        let start = index * 128 * 1024;
        let segment = TaskSegmentRecord {
            id: format!("resumed-{index}"),
            task_id: task.id.clone(),
            file_id: None,
            unit_kind: "http_range".into(),
            range_start: start,
            range_end: start + 128 * 1024 - 1,
            downloaded_until: if index == 0 { 8192 } else { start },
            speed_bps: 0,
            status: SegmentStatus::Pending,
            retry_count: 0,
            last_error: None,
        };
        db::insert_segment_record(&pool, &segment)
            .await
            .expect("segment");
    }
    let mut partial = vec![0; payload.len()];
    partial[..8192].copy_from_slice(&payload[..8192]);
    std::fs::write(&paths.temp, partial).expect("partial file");
    let database_file: String = sqlx::query("PRAGMA database_list")
        .fetch_one(&pool)
        .await
        .expect("db path")
        .get("file");
    pool.close().await;
    let pool = db::connect(std::path::Path::new(&database_file))
        .await
        .expect("reopen")
        .pool;
    let mut context = common::headless_download_context(
        pool.clone(),
        task,
        tokio_util::sync::CancellationToken::new(),
    );
    context.connection_limit = 2;
    context.request_headers = db::resolve_task_request_profile_headers(&pool, "resumed")
        .await
        .expect("restored profile");
    engine.download(context).await.expect("download");
    assert_eq!(std::fs::read(&paths.final_path).expect("result"), *payload);
    let requests = captured.lock().expect("capture");
    assert!(
        requests.iter().any(|raw| raw.contains("bytes=8192-")),
        "resume must start at the durable checkpoint"
    );
    assert!(requests.iter().filter(|raw| raw.starts_with("GET")).count() >= 2);
    for request in requests.iter() {
        let lower = request.to_ascii_lowercase();
        for expected in [
            "user-agent: profileregression/1",
            "referer: https://example.com/page",
            "accept-language: zh-cn",
            "cookie: session=profile-secret",
            "x-token: profile-api-secret",
        ] {
            assert!(lower.contains(expected), "missing {expected}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_origin_redirect_and_saved_final_url_never_receive_secret_headers() {
    common::install_test_secret_key();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let payload = Arc::new(vec![7; 2048]);
    let target = capture_file_server(payload.clone(), captured.clone());
    let redirect_url = format!("{}/file", target.base_url);
    let origin_requests = Arc::new(Mutex::new(Vec::new()));
    let origin_capture = origin_requests.clone();
    let origin = common::TestServer::start(move |mut stream| {
        let mut buf = [0; 16384];
        let read = stream.read(&mut buf).unwrap_or(0);
        origin_capture
            .lock()
            .expect("capture")
            .push(String::from_utf8_lossy(&buf[..read]).into_owned());
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: {redirect_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("redirect");
    });
    let url = format!("{}/redirect", origin.base_url);
    let engine = HttpEngine::new().expect("engine");
    let headers = normalize_task_request_profile(&url, &profile()).expect("headers");
    let probe = engine
        .probe_with_headers(&url, &headers)
        .await
        .expect("redirect probe");
    assert!(probe.final_url.starts_with(&target.base_url));
    let (_db, pool) = common::test_pool("profile-final-url").await;
    let paths = common::TestPaths::new("profile-final");
    let mut task = common::download_task(
        "final-url",
        url,
        "http",
        "file.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    task.final_url = Some(probe.final_url);
    db::insert_task_record(&pool, &task).await.expect("task");
    let mut context =
        common::headless_download_context(pool, task, tokio_util::sync::CancellationToken::new());
    context.request_headers = headers;
    engine
        .download(context)
        .await
        .expect("download from saved final url");
    assert_eq!(std::fs::read(&paths.final_path).expect("file"), *payload);
    assert!(origin_requests
        .lock()
        .expect("capture")
        .iter()
        .any(|raw| raw.contains("profile-api-secret")));
    for request in captured.lock().expect("capture").iter() {
        let lower = request.to_ascii_lowercase();
        assert!(
            !lower.contains("cookie:")
                && !lower.contains("x-token:")
                && !lower.contains("authorization:")
        );
        assert!(lower.contains("user-agent: profileregression/1"));
    }
}

#[tokio::test]
async fn same_origin_redirect_keeps_profile_headers() {
    let capture = Arc::new(Mutex::new(Vec::new()));
    let requests = capture.clone();
    let server = common::TestServer::start(move |mut stream| {
        let mut buf = [0; 16384];
        let read = stream.read(&mut buf).unwrap_or(0);
        let raw = String::from_utf8_lossy(&buf[..read]).into_owned();
        let is_redirect = raw.contains(" /redirect ");
        requests.lock().expect("capture").push(raw);
        if is_redirect {
            write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: /file\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("redirect");
        } else {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n"
            )
            .expect("response");
        }
    });
    let url = format!("{}/redirect", server.base_url);
    HttpEngine::new()
        .expect("engine")
        .probe_with_headers(
            &url,
            &normalize_task_request_profile(&url, &profile()).expect("headers"),
        )
        .await
        .expect("same origin probe");
    assert_eq!(capture.lock().expect("capture").len(), 2);
    assert!(capture
        .lock()
        .expect("capture")
        .iter()
        .all(|raw| raw.contains("profile-api-secret") && raw.contains("session=profile-secret")));
}

#[cfg(debug_assertions)]
fn creation_state(pool: sqlx::SqlitePool) -> tauri_app_lib::AppState {
    use tauri_app_lib::{
        download::{EngineRegistry, GlobalSpeedLimiter},
        scheduler::Scheduler,
    };
    let downloads = Arc::default();
    let request_headers = Arc::default();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let engine_registry = Arc::new(EngineRegistry::new().expect("registry"));
    let task_runtime_locks = Arc::default();
    let scheduler = Arc::new(Scheduler::new(
        Arc::clone(&downloads),
        Arc::clone(&request_headers),
        speed_limiter.clone(),
        engine_registry.clone(),
        Arc::clone(&task_runtime_locks),
    ));
    tauri_app_lib::AppState {
        pool,
        downloads,
        request_headers,
        speed_limiter,
        engine_registry,
        task_runtime_locks,
        scheduler,
        browser_realtime: tauri_app_lib::browser_realtime::BrowserRealtimeState::new(),
        quit_requested: Arc::default(),
        lifecycle_gate: Arc::default(),
        lifecycle: Arc::default(),
        active_supervisors: Arc::default(),
        close_request_pending: Arc::default(),
    }
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn creation_uses_profile_reprobes_stale_negotiation_and_rolls_back_profile_write_failure() {
    use tauri_app_lib::{
        commands::tasks::create_task_headless, download::network_policy::TaskSource,
    };
    common::install_test_secret_key();
    let payload = Arc::new(vec![1; 1024]);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let server = capture_file_server(payload, captured.clone());
    let (_db, pool) = common::test_pool("profile-creation").await;
    let state = creation_state(pool.clone());
    let paths = common::TestPaths::new("profile-creation");
    let url = format!("{}/file", server.base_url);
    let authorization = db::create_network_authorization(&pool, TaskSource::Manual, &url)
        .await
        .expect("authorization");
    let fields = serde_json::json!({
        "url": url, "saveDir": paths.final_path.parent().expect("parent"),
        "networkAuthorizationId": authorization.id, "sourceKind": "manual", "allowDuplicate": true,
        "requestProfile": profile(),
        "probeSnapshot": { "inputUrl": url, "finalUrl": url, "fileName": "obsolete.bin", "protocol": "http",
            "taskKind": "single_file", "capabilities": { "supportsResume": true, "supportsParallel": true, "supportsMultiFile": false },
            "files": [{ "relativePath": "obsolete.bin", "size": "9", "contentType": null }], "totalSize": "9",
            "sourceKey": "127.0.0.1", "contentType": null, "etag": null, "lastModified": null,
            "hlsVariants": [], "hlsAudioTracks": [], "hlsSubtitleTracks": [], "probedAt": chrono::Utc::now().to_rfc3339() }
    });
    let task = create_task_headless(
        &state,
        serde_json::from_value(fields.clone()).expect("input"),
    )
    .await
    .expect("create");
    assert_eq!(task.file_name, "file.bin");
    let headers = db::resolve_task_request_profile_headers(&pool, &task.id)
        .await
        .expect("persisted creation profile");
    assert_eq!(
        headers,
        normalize_task_request_profile(&url, &profile()).expect("normalized")
    );
    assert!(captured
        .lock()
        .expect("capture")
        .iter()
        .any(|request| request.contains("profile-api-secret")));
    sqlx::query("CREATE TRIGGER reject_profile BEFORE INSERT ON task_request_profiles BEGIN SELECT RAISE(ABORT, 'profile persistence failure'); END")
        .execute(&pool).await.expect("inject persistence failure");
    let mut second = fields;
    let authorization = db::create_network_authorization(&pool, TaskSource::Manual, &url)
        .await
        .expect("authorization");
    second["networkAuthorizationId"] = authorization.id.into();
    create_task_headless(&state, serde_json::from_value(second).expect("input"))
        .await
        .expect_err("creation must fail atomically");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks")
        .fetch_one(&pool)
        .await
        .expect("tasks count");
    assert_eq!(
        count, 1,
        "failed profile persistence must not leave a queued task"
    );
}
