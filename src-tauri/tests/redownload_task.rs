//! Regression coverage for completed-task redownload configuration copying.

mod common;

use std::{io::Read, sync::Arc};

use common::TestPaths;
use tauri_app_lib::{
    commands::tasks::redownload_task_headless,
    db,
    download::{EngineRegistry, GlobalSpeedLimiter},
    models::{
        BrowserKind, TaskPriority, TaskProxyMode, TaskRequestHeaderInput, TaskRequestProfileInput,
        TaskStatus,
    },
    scheduler::Scheduler,
    AppState,
};
fn test_state(pool: sqlx::SqlitePool) -> AppState {
    let downloads = Arc::default();
    let request_headers = Arc::default();
    let speed_limiter = GlobalSpeedLimiter::disabled();
    let engine_registry = Arc::new(EngineRegistry::new().expect("engine registry"));
    let task_runtime_locks = Arc::default();
    let scheduler = Arc::new(Scheduler::new(
        Arc::clone(&downloads),
        Arc::clone(&request_headers),
        speed_limiter.clone(),
        engine_registry.clone(),
        Arc::clone(&task_runtime_locks),
    ));
    AppState {
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

fn file_server(payload: Arc<Vec<u8>>) -> common::TestServer {
    common::TestServer::start(move |mut stream| {
        let mut request = [0; 32 * 1024];
        let read = stream.read(&mut request).unwrap_or(0);
        if read == 0 {
            return;
        }
        let raw = String::from_utf8_lossy(&request[..read]);
        let method = raw.split_whitespace().next().unwrap_or("GET");
        let range = raw.lines().find_map(common::http::parse_range);
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

fn profile() -> TaskRequestProfileInput {
    TaskRequestProfileInput {
        user_agent: Some("Redownload/1".into()),
        referer: Some("https://example.com/source".into()),
        custom_headers: vec![TaskRequestHeaderInput {
            name: "X-Redownload-Profile".into(),
            value: "profile-value".into(),
        }],
    }
}

async fn persist_profile(pool: &sqlx::SqlitePool, task_id: &str, url: &str) {
    let prepared = db::prepare_task_request_profile(url, &profile()).expect("prepare profile");
    let mut conn = pool.acquire().await.expect("connection");
    db::save_task_request_profile(&mut conn, task_id, &prepared)
        .await
        .expect("save profile");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redownload_copies_persisted_configuration_and_browser_headers() {
    common::install_test_secret_key();
    let payload = Arc::new(
        (0..4096)
            .map(|index| (index % 251) as u8)
            .collect::<Vec<_>>(),
    );
    let server = file_server(payload.clone());
    let (_db, pool) = common::test_pool("redownload-config").await;
    let state = test_state(pool.clone());
    let paths = TestPaths::new("redownload-config");
    let url = format!("{}/file", server.base_url);
    let authorization = db::create_network_authorization(
        &pool,
        tauri_app_lib::download::network_policy::TaskSource::Manual,
        &url,
    )
    .await
    .expect("authorization");
    let mut task = common::download_task(
        "completed-source",
        url.clone(),
        "http",
        "file.bin",
        payload.len() as i64,
        &paths,
        true,
    );
    task.status = TaskStatus::Completed;
    task.downloaded_bytes = payload.len() as i64;
    task.health_summary = Some("Completed".into());
    task.task_speed_limit_bps = Some("1234".into());
    task.priority = TaskPriority::High;
    task.category_key = Some("work".into());
    task.obey_schedule = false;
    db::insert_task_record(&pool, &task)
        .await
        .expect("source task");
    db::save_task_network_policy(&pool, &task.id, &authorization.policy)
        .await
        .expect("source policy");
    persist_profile(&pool, &task.id, &url).await;
    db::upsert_task_request_headers(
        &pool,
        &task.id,
        &[
            ("cookie".into(), "session=browser-value".into()),
            ("user-agent".into(), "Browser/99".into()),
        ],
        Some(BrowserKind::Chrome),
    )
    .await
    .expect("browser headers");
    let source_profile_expiry: Option<String> = sqlx::query_scalar(
        "SELECT sensitive_expires_at FROM task_request_profiles WHERE task_id = ?",
    )
    .bind(&task.id)
    .fetch_one(&pool)
    .await
    .expect("profile expiry");
    let source_browser_expiry: String =
        sqlx::query_scalar("SELECT expires_at FROM task_request_headers WHERE task_id = ?")
            .bind(&task.id)
            .fetch_one(&pool)
            .await
            .expect("browser expiry");
    db::upsert_task_credentials(&pool, &task.id, "http", "user", "password", None, None)
        .await
        .expect("credentials");
    db::upsert_task_proxy_settings(
        &pool,
        tauri_app_lib::models::TaskProxySettingsInput {
            task_id: task.id.clone(),
            mode: TaskProxyMode::Off,
            proxy_url: None,
            proxy_username: None,
            proxy_password: None,
            clear_proxy_password: None,
            no_proxy: Some("localhost,127.0.0.1".into()),
        },
    )
    .await
    .expect("proxy settings");

    let redownloaded = redownload_task_headless(&state, &task.id)
        .await
        .expect("redownload");
    assert_ne!(redownloaded.id, task.id);
    assert_eq!(redownloaded.save_dir, task.save_dir);
    assert!(redownloaded.file_name.starts_with("file"));
    assert_eq!(redownloaded.task_speed_limit_bps, task.task_speed_limit_bps);
    assert_eq!(redownloaded.priority, task.priority);
    assert_eq!(redownloaded.category_key, task.category_key);
    assert_eq!(redownloaded.obey_schedule, task.obey_schedule);
    assert_eq!(redownloaded.status, TaskStatus::Queued);

    let copied_profile = db::resolve_task_request_profile_headers(&pool, &redownloaded.id)
        .await
        .expect("copied profile");
    assert!(copied_profile
        .iter()
        .any(|(name, value)| { name == "x-redownload-profile" && value == "profile-value" }));
    assert_eq!(
        db::resolve_task_request_headers(&pool, &redownloaded.id)
            .await
            .expect("copied browser headers"),
        vec![
            ("cookie".into(), "session=browser-value".into()),
            ("user-agent".into(), "Browser/99".into())
        ]
    );
    let source_browser: Option<String> =
        sqlx::query_scalar("SELECT source_browser FROM task_request_headers WHERE task_id = ?")
            .bind(&redownloaded.id)
            .fetch_one(&pool)
            .await
            .expect("browser source");
    assert_eq!(source_browser.as_deref(), Some("chrome"));
    let copied_profile_expiry: Option<String> = sqlx::query_scalar(
        "SELECT sensitive_expires_at FROM task_request_profiles WHERE task_id = ?",
    )
    .bind(&redownloaded.id)
    .fetch_one(&pool)
    .await
    .expect("copied profile expiry");
    assert_eq!(copied_profile_expiry, source_profile_expiry);
    let copied_browser_expiry: String =
        sqlx::query_scalar("SELECT expires_at FROM task_request_headers WHERE task_id = ?")
            .bind(&redownloaded.id)
            .fetch_one(&pool)
            .await
            .expect("copied browser expiry");
    assert_eq!(copied_browser_expiry, source_browser_expiry);

    let copied_credentials = db::resolve_task_credentials(&pool, &redownloaded.id)
        .await
        .expect("copied credentials")
        .expect("credentials row");
    assert_eq!(copied_credentials.username, "user");
    assert_eq!(copied_credentials.password, "password");
    let copied_proxy = db::get_task_proxy_settings(&pool, &redownloaded.id)
        .await
        .expect("copied proxy");
    assert_eq!(copied_proxy.mode, TaskProxyMode::Off);
    assert_eq!(copied_proxy.no_proxy, "localhost,127.0.0.1");

    let copied_policy = db::task_network_policy(&pool, &redownloaded.id)
        .await
        .expect("copied policy");
    assert_eq!(copied_policy, authorization.policy);
    assert!(db::get_task_record(&pool, &task.id)
        .await
        .expect("source remains")
        .is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redownload_without_legacy_policy_does_not_create_empty_authorization() {
    let payload = Arc::new(vec![7; 128]);
    let server = file_server(payload.clone());
    let (_db, pool) = common::test_pool("redownload-no-policy").await;
    let state = test_state(pool.clone());
    let paths = TestPaths::new("redownload-no-policy");
    let url = format!("{}/file", server.base_url);
    let mut task = common::download_task(
        "legacy-source",
        url,
        "http",
        "file.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    task.status = TaskStatus::Completed;
    task.downloaded_bytes = payload.len() as i64;
    task.health_summary = Some("Completed".into());
    db::insert_task_record(&pool, &task)
        .await
        .expect("source task");

    let error = redownload_task_headless(&state, &task.id)
        .await
        .expect_err("private legacy target must be re-authorized");
    assert!(error.contains("intranet_target_blocked"), "{error}");
    let authorization_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM network_authorizations")
            .fetch_one(&pool)
            .await
            .expect("authorization count");
    assert_eq!(authorization_count, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM tasks")
            .fetch_one(&pool)
            .await
            .expect("task count"),
        1
    );
}
