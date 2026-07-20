//! FUN-03: auth-header recovery helpers and same-URL header refresh.

use std::time::{SystemTime, UNIX_EPOCH};

use tauri_app_lib::{
    commands::browser::is_auth_header_recovery_candidate,
    db,
    models::task::now_iso,
    models::{HashVerificationStatus, TaskKind, TaskPriority, TaskRecord, TaskStatus},
    state_machine,
};

mod common;

fn sample_task(id: &str, status: TaskStatus, error_code: Option<&str>) -> TaskRecord {
    let now = now_iso();
    TaskRecord {
        id: id.to_string(),
        url: format!("https://example.com/files/{id}.bin"),
        final_url: None,
        protocol: "https".to_string(),
        task_kind: TaskKind::SingleFile,
        file_name: format!("{id}.bin"),
        save_dir: std::env::temp_dir().to_string_lossy().to_string(),
        temp_path: None,
        final_path: None,
        total_size: 1024,
        downloaded_bytes: 0,
        status,
        etag: None,
        last_modified: None,
        content_type: None,
        supports_resume: true,
        supports_parallel: true,
        supports_multi_file: false,
        source_key: format!("https://example.com/files/{id}.bin"),
        connection_count: 0,
        speed_bps: 0,
        task_speed_limit_bps: None,
        priority: TaskPriority::Normal,
        queue_position: 0,
        category_key: None,
        obey_schedule: true,
        health_summary: None,
        error_message: error_code.map(|code| format!("error:{code}")),
        error_code: error_code.map(str::to_string),
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

async fn test_pool(label: &str) -> sqlx::SqlitePool {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("vibe-fun03-{label}-{id}.sqlite"));
    db::connect(&path)
        .await
        .expect("database connect with migrations")
        .pool
}

#[test]
fn fun03_recovery_candidate_only_auth_attention_or_failed() {
    assert!(is_auth_header_recovery_candidate(
        TaskStatus::NeedsAttention,
        "auth_headers_expired"
    ));
    assert!(is_auth_header_recovery_candidate(
        TaskStatus::Failed,
        "auth_headers_unavailable"
    ));
    assert!(!is_auth_header_recovery_candidate(
        TaskStatus::Downloading,
        "auth_headers_expired"
    ));
    assert!(!is_auth_header_recovery_candidate(
        TaskStatus::Queued,
        "auth_headers_expired"
    ));
    assert!(!is_auth_header_recovery_candidate(
        TaskStatus::NeedsAttention,
        "remote_changed"
    ));
}

#[tokio::test]
async fn fun03_expired_headers_refresh_and_requeue_same_task() {
    common::install_test_secret_key();
    let pool = test_pool("refresh").await;
    let task = sample_task(
        "fun03-expired",
        TaskStatus::NeedsAttention,
        Some("auth_headers_expired"),
    );
    db::insert_task_record(&pool, &task).await.expect("insert");

    let headers = vec![
        ("cookie".to_string(), "session=abc".to_string()),
        ("referer".to_string(), "https://example.com/".to_string()),
    ];
    db::upsert_task_request_headers(&pool, &task.id, &headers, None)
        .await
        .expect("upsert headers");

    let app: Option<tauri::AppHandle> = None;
    state_machine::transition_task_with_runtime_state(
        &app,
        &pool,
        &task.id,
        TaskStatus::Queued,
        0,
        0,
        Some("Queued"),
        Some("auth_headers_refreshed"),
        Some("Browser headers refreshed"),
        tauri_app_lib::models::SegmentStatus::Pending,
        None,
        None,
    )
    .await
    .expect("requeue");

    let updated = db::get_task_record(&pool, &task.id)
        .await
        .expect("query")
        .expect("exists");
    assert_eq!(updated.status, TaskStatus::Queued);

    let resolved = db::resolve_task_request_headers(&pool, &task.id)
        .await
        .expect("resolve headers");
    assert_eq!(resolved.len(), 2);
    assert!(resolved.iter().any(|(name, _)| name == "cookie"));

    // Active same-URL task must remain the duplicate winner and is not a recovery candidate.
    let active = sample_task("fun03-active", TaskStatus::Downloading, None);
    let mut active = active;
    active.url = task.url.clone();
    active.source_key = task.source_key.clone();
    db::insert_task_record(&pool, &active)
        .await
        .expect("insert active");
    let duplicate = db::find_duplicate_task_record(&pool, &task.url, None, None)
        .await
        .expect("find duplicate")
        .expect("exists");
    assert_eq!(duplicate.id, active.id);
    assert!(!is_auth_header_recovery_candidate(
        duplicate.status,
        duplicate.error_code.as_deref().unwrap_or("")
    ));

    pool.close().await;
}
