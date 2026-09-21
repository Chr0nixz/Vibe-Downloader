//! §3.9 Browser Integration Center backend coverage: handoff history
//! queries, the dry-run handoff validation (validated / duplicate / failed
//! with zero side effects), the expired auth-header task listing, and the
//! native host self-check graceful-unavailable path.

mod common;

use tauri_app_lib::{
    commands::browser::{run_browser_native_host_self_check, validate_browser_handoff_with_state},
    db,
    download::EngineRegistry,
    models::{BrowserHandoffInput, BrowserKind, TaskStatus},
};

fn handoff_input(request_id: &str, url: &str) -> BrowserHandoffInput {
    BrowserHandoffInput {
        version: 1,
        request_id: request_id.to_string(),
        browser: BrowserKind::Chrome,
        action: "download_url".to_string(),
        url: url.to_string(),
        source: None,
        browser_download_id: None,
        page_url: None,
        referrer: None,
        user_agent: None,
        suggested_file_name: None,
        total_bytes: None,
        mime: None,
        headers_available: None,
        header_consent_state: None,
        forwarded_headers: None,
    }
}

#[tokio::test]
async fn handoff_history_returns_recent_window_and_totals() {
    let (_guard, pool) = common::test_pool("browser-center-history").await;

    // Inserted oldest → newest; equal `created_at` timestamps (same
    // now_iso() precision) fall back to request_id DESC, which is also
    // newest-inserted-first for these ids.
    let rows: [(&str, BrowserKind, &str, Option<&str>); 3] = [
        ("req-0", BrowserKind::Chrome, "received", None),
        (
            "req-1",
            BrowserKind::Firefox,
            "failed",
            Some("Browser handoff only supports HTTP and HTTPS URLs."),
        ),
        ("req-2", BrowserKind::Edge, "received", None),
    ];
    for (request_id, browser, status, error) in rows {
        db::insert_browser_message(
            &pool,
            request_id,
            browser,
            "https://example.com/file.bin",
            status,
            error,
        )
        .await
        .expect("insert browser message");
    }

    let window = db::recent_browser_messages(&pool, 20)
        .await
        .expect("history window");
    let ids: Vec<&str> = window.iter().map(|row| row.request_id.as_str()).collect();
    assert_eq!(ids, vec!["req-2", "req-1", "req-0"], "newest first");
    assert_eq!(window[1].browser, BrowserKind::Firefox);
    assert_eq!(window[1].status, "failed");
    assert!(window[1].error_message.is_some());
    // Stored URLs are SEC-06 query-stripped copies.
    assert!(window[0].url.starts_with("https://example.com/"));

    let limited = db::recent_browser_messages(&pool, 2)
        .await
        .expect("limited window");
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].request_id, "req-2");

    let summary = db::browser_message_summary(&pool).await.expect("summary");
    assert_eq!(summary.received_count, 2);
    assert_eq!(summary.failed_count, 1);
    assert!(summary.last_created_at.is_some());

    pool.close().await;
}

#[tokio::test]
async fn handoff_history_skips_rows_with_unknown_browser_kinds() {
    let (_guard, pool) = common::test_pool("browser-center-history-unknown").await;
    db::insert_browser_message(
        &pool,
        "req-known",
        BrowserKind::Chrome,
        "https://example.com/a.bin",
        "received",
        None,
    )
    .await
    .expect("insert known row");
    // A newer build may write a kind this binary cannot parse; the history
    // window skips it instead of rendering a broken entry.
    sqlx::query(
        "INSERT INTO browser_messages (request_id, browser, url, status, created_at) \
         VALUES ('req-future', 'future_browser', 'https://example.com/b.bin', 'received', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect("insert unknown-kind row");

    let window = db::recent_browser_messages(&pool, 20)
        .await
        .expect("history window");
    assert_eq!(window.len(), 1);
    assert_eq!(window[0].request_id, "req-known");

    pool.close().await;
}

#[tokio::test]
async fn dry_run_validates_duplicate_and_rejects_without_side_effects() {
    let (_guard, pool) = common::test_pool("browser-center-dryrun").await;
    let registry = EngineRegistry::new().expect("engine registry");
    // Public IP literal: keeps the SSRF pre-flight deterministic (no DNS).
    let valid_url = "https://93.184.216.34/file.bin";

    let ok =
        validate_browser_handoff_with_state(&pool, &registry, handoff_input("req-ok", valid_url))
            .await
            .expect("dry-run call succeeds");
    assert_eq!(ok.status, "validated");
    assert!(ok.task.is_none());
    assert!(ok.error_message.is_none());

    // The duplicate check is part of the dry-run: the id already exists in
    // browser_messages (seeded directly, since the dry-run never writes).
    db::insert_browser_message(
        &pool,
        "req-dup",
        BrowserKind::Chrome,
        valid_url,
        "received",
        None,
    )
    .await
    .expect("seed duplicate row");
    let dup =
        validate_browser_handoff_with_state(&pool, &registry, handoff_input("req-dup", valid_url))
            .await
            .expect("dry-run call succeeds");
    assert_eq!(dup.status, "duplicate");
    assert!(dup.error_message.is_none());

    let scheme = validate_browser_handoff_with_state(
        &pool,
        &registry,
        handoff_input("req-ftp", "ftp://93.184.216.34/file.bin"),
    )
    .await
    .expect("dry-run call succeeds");
    assert_eq!(scheme.status, "failed");
    assert!(
        scheme
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("HTTP and HTTPS")),
        "unexpected error: {scheme:?}"
    );

    let credentials = validate_browser_handoff_with_state(
        &pool,
        &registry,
        handoff_input("req-cred", "https://user:pass@93.184.216.34/file.bin"),
    )
    .await
    .expect("dry-run call succeeds");
    assert_eq!(credentials.status, "failed");
    assert!(
        credentials
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("embedded credentials")),
        "unexpected error: {credentials:?}"
    );

    // Default capture settings keep allow_intranet_handoff off, so loopback
    // targets stay rejected at the boundary.
    let private = validate_browser_handoff_with_state(
        &pool,
        &registry,
        handoff_input("req-private", "http://127.0.0.1:8080/file.bin"),
    )
    .await
    .expect("dry-run call succeeds");
    assert_eq!(private.status, "failed");
    assert!(
        private
            .error_message
            .as_deref()
            .is_some_and(|message| message.contains("private or reserved")),
        "unexpected error: {private:?}"
    );

    // Zero side effects: only the seeded duplicate row exists, and the
    // dry-run never created a task.
    let summary = db::browser_message_summary(&pool).await.expect("summary");
    assert_eq!(summary.received_count, 1);
    assert_eq!(summary.failed_count, 0);
    let tasks = db::list_task_records(&pool).await.expect("task list");
    assert!(tasks.is_empty(), "dry-run must not create tasks");

    pool.close().await;
}

#[tokio::test]
async fn expired_auth_header_listing_filters_status_and_code() {
    let (_guard, pool) = common::test_pool("browser-center-expired").await;
    let paths = common::TestPaths::new("browser-center-expired");
    let seed_task = |id: &str, status: TaskStatus, code: &str| {
        let mut task = common::download_task(
            id,
            format!("https://93.184.216.34/{id}.bin"),
            "http",
            &format!("{id}.bin"),
            0,
            &paths,
            true,
        );
        let final_dir = std::path::Path::new(&paths.final_path)
            .parent()
            .expect("final directory");
        task.final_path = Some(
            final_dir
                .join(format!("{id}.bin"))
                .to_string_lossy()
                .to_string(),
        );
        task.status = status;
        task.error_code = Some(code.to_string());
        task
    };
    db::insert_task_record(
        &pool,
        &seed_task("t1", TaskStatus::Failed, "auth_headers_expired"),
    )
    .await
    .expect("seed t1");
    db::insert_task_record(
        &pool,
        &seed_task("t2", TaskStatus::NeedsAttention, "auth_headers_unavailable"),
    )
    .await
    .expect("seed t2");
    // Same code but still actively downloading — recovery must not touch it.
    db::insert_task_record(
        &pool,
        &seed_task("t3", TaskStatus::Downloading, "auth_headers_expired"),
    )
    .await
    .expect("seed t3");
    // Failed but with an unrelated code — belongs to the Recovery Center.
    db::insert_task_record(
        &pool,
        &seed_task("t4", TaskStatus::Failed, "disk_write_failed"),
    )
    .await
    .expect("seed t4");

    let rows = db::list_expired_auth_header_tasks(&pool)
        .await
        .expect("expired listing");
    let mut ids: Vec<&str> = rows.iter().map(|row| row.task_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["t1", "t2"]);

    let t1 = rows.iter().find(|row| row.task_id == "t1").expect("t1");
    assert_eq!(t1.error_code, "auth_headers_expired");
    assert_eq!(t1.status, "failed");
    assert_eq!(t1.file_name, "t1.bin");
    assert!(t1.url.starts_with("https://93.184.216.34/"));

    pool.close().await;
}

#[tokio::test]
async fn self_check_reports_unavailable_without_sibling_host() {
    // Test binaries live in target/<profile>/deps where no vibe-native-host
    // binary sits next to the current exe, so the structured "unavailable"
    // result — never a hard error — is the deterministic outcome here.
    let result = run_browser_native_host_self_check()
        .await
        .expect("self-check never hard-fails");
    assert!(
        !result.available,
        "unexpected available self-check in test env: {result:?}"
    );
    assert!(!result.ok);
    assert!(result.error_message.is_some());
}
