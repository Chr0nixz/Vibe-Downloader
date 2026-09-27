//! Recovery Center gating and history tests (feature proposal §3.3).
//!
//! The batch/credential command bodies are thin over these pure gates plus
//! the per-task runtime lock (R-2.3), and the latter needs a real
//! `AppHandle<Wry>` to exercise end-to-end. The decision logic is asserted
//! here; the ARC-41 start-failure routing it depends on is covered by the
//! scheduler in-crate tests (`arc41_*`).

mod common;

use tauri_app_lib::commands::recovery::{
    bulk_resolution_gate, credentials_update_gate, normalize_recovery_source, BulkGate,
    CredentialsGateError,
};
use tauri_app_lib::db;
use tauri_app_lib::models::recovery::{BulkRecoveryAction, RecoveryHistoryRecord};
use tauri_app_lib::models::{RecoveryAction, TaskStatus};

#[test]
fn bulk_gate_proceeds_for_recoverable_failures() {
    assert_eq!(
        bulk_resolution_gate(&TaskStatus::Failed, Some("http_status")),
        BulkGate::Proceed
    );
    // A user-action conflict is recoverable through choose_another_name or
    // folder, but not through a blind bulk retry (see the dedicated skip
    // test below).
    assert_eq!(
        bulk_resolution_gate(&TaskStatus::Failed, None),
        BulkGate::Proceed
    );
    assert_eq!(
        bulk_resolution_gate(&TaskStatus::NeedsAttention, None),
        BulkGate::Proceed
    );
}

#[test]
fn bulk_gate_skips_publish_path_conflicts() {
    // finalize_download_file never clobbers or auto-renames, so the conflict
    // persists until the user acts; a bulk re-queue would just re-fail.
    assert_eq!(
        bulk_resolution_gate(&TaskStatus::NeedsAttention, Some("final_path_conflict")),
        BulkGate::Skip
    );
    assert_eq!(
        bulk_resolution_gate(&TaskStatus::Failed, Some("final_path_conflict")),
        BulkGate::Skip
    );
}

#[test]
fn normalize_recovery_source_bounds_and_defaults() {
    assert_eq!(
        normalize_recovery_source("recovery_center"),
        "recovery_center"
    );
    assert_eq!(normalize_recovery_source("  manual  "), "manual");
    assert_eq!(normalize_recovery_source(""), "manual");
    assert_eq!(normalize_recovery_source("   "), "manual");
    let long = "x".repeat(200);
    assert_eq!(normalize_recovery_source(&long), "x".repeat(32));
    // Control characters and newlines do not extend the stored value.
    assert_eq!(
        normalize_recovery_source(
            "auto
more"
        ),
        "auto
more"
    );
}

#[test]
fn bulk_gate_skips_restart_required_and_non_failures() {
    for code in [
        "remote_changed",
        "resume_unavailable",
        "temp_file_missing",
        "temp_file_smaller_than_progress",
    ] {
        assert_eq!(
            bulk_resolution_gate(&TaskStatus::NeedsAttention, Some(code)),
            BulkGate::Skip,
            "{code} must go through the per-task destructive playbook"
        );
    }
    for status in [
        TaskStatus::Queued,
        TaskStatus::Downloading,
        TaskStatus::Retrying,
        TaskStatus::Completed,
        TaskStatus::Paused,
        TaskStatus::WaitingNetwork,
    ] {
        assert_eq!(
            bulk_resolution_gate(&status, None),
            BulkGate::Skip,
            "{status:?} is not a recovery-surface failure"
        );
    }
}

#[test]
fn bulk_actions_match_recovery_action_spelling() {
    // History rows store the action string; the frontend label table is
    // keyed by the RecoveryAction spellings, so the two must stay aligned.
    assert_eq!(
        BulkRecoveryAction::Retry.as_str(),
        RecoveryAction::Retry.as_str()
    );
    assert_eq!(
        BulkRecoveryAction::RetryLater.as_str(),
        RecoveryAction::RetryLater.as_str()
    );
}

#[test]
fn credential_gate_blocks_unsupported_protocols_and_busy_tasks() {
    assert!(credentials_update_gate("sftp", &TaskStatus::NeedsAttention).is_ok());
    assert!(credentials_update_gate("webdavs", &TaskStatus::Failed).is_ok());
    assert!(credentials_update_gate("ftps", &TaskStatus::Paused).is_ok());
    // HTTP family: Basic Auth tasks and the derived engines consume the same
    // task_credentials store, so password rotation must be repairable in place.
    for protocol in ["http", "https", "hls", "dash", "metalink"] {
        assert!(
            credentials_update_gate(protocol, &TaskStatus::NeedsAttention).is_ok(),
            "{protocol} should accept credential updates"
        );
    }
    // BT/magnet tasks have no credential channel; the gate must still refuse.
    for protocol in ["bt", "magnet"] {
        assert_eq!(
            credentials_update_gate(protocol, &TaskStatus::NeedsAttention),
            Err(CredentialsGateError::UnsupportedProtocol),
            "{protocol} should stay unsupported"
        );
    }
    assert_eq!(
        credentials_update_gate("ftp", &TaskStatus::Downloading),
        Err(CredentialsGateError::TaskBusy)
    );
    assert_eq!(
        credentials_update_gate("ftp", &TaskStatus::Retrying),
        Err(CredentialsGateError::TaskBusy)
    );
    // The busy gate must apply to the HTTP family too, not just FTP.
    assert_eq!(
        credentials_update_gate("https", &TaskStatus::Downloading),
        Err(CredentialsGateError::TaskBusy)
    );
}

fn test_db_path(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "vibe-recovery-tests-{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ))
}

fn history_record(id: &str, created_at: &str) -> RecoveryHistoryRecord {
    RecoveryHistoryRecord {
        id: id.to_string(),
        task_id: format!("task-{id}"),
        task_file_name: Some("file.bin".to_string()),
        action: "retry".to_string(),
        source: "recovery_center".to_string(),
        error_code: Some("http_status".to_string()),
        created_at: created_at.to_string(),
    }
}

#[tokio::test]
async fn recovery_history_lists_newest_first() {
    let pool = db::connect(&test_db_path("list"))
        .await
        .expect("test pool")
        .pool;

    db::insert_recovery_record(&pool, &history_record("a", "2026-09-13T10:00:00Z"))
        .await
        .expect("insert a");
    db::insert_recovery_record(&pool, &history_record("b", "2026-09-13T11:00:00Z"))
        .await
        .expect("insert b");

    let history = db::list_recovery_history(&pool, 10).await.expect("list");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, "b", "newest first");
    assert_eq!(history[0].source, "recovery_center");
    assert_eq!(history[0].error_code.as_deref(), Some("http_status"));

    // The limit clamps the result from the caller side too.
    let one = db::list_recovery_history(&pool, 1).await.expect("list one");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].id, "b");

    pool.close().await;
}

#[tokio::test]
async fn recovery_history_is_bounded_by_prune_on_insert() {
    let pool = db::connect(&test_db_path("bound"))
        .await
        .expect("test pool")
        .pool;

    // Padded stamps keep lexical order aligned with insertion order.
    for index in 0..220 {
        let stamp = format!("2026-09-13T10:{index:04}");
        db::insert_recovery_record(&pool, &history_record(&index.to_string(), &stamp))
            .await
            .expect("insert");
    }

    let history = db::list_recovery_history(&pool, 1000).await.expect("list");
    assert_eq!(history.len(), 200, "the log must stay bounded");
    assert_eq!(history[0].id, "219", "the newest insert survives");
    assert_eq!(history.last().expect("tail").id, "20", "oldest pruned");

    pool.close().await;
}

/// FUN-31: an HTTP task must accept a credential update the same way an
/// FTP/SFTP/WebDAV task does — the encrypted row lands in task_credentials
/// and resolves back to the same Basic Auth secret.
#[tokio::test]
async fn http_task_credentials_round_trip_through_encrypted_store() {
    common::install_test_secret_key();
    let (_db, pool) = common::test_pool("http-credentials").await;

    let paths = common::TestPaths::new("http-credentials");
    let task = common::download_task(
        "http-cred-task",
        "https://example.com/file.bin".to_string(),
        "https",
        "file.bin",
        1024,
        &paths,
        false,
    );
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert task");

    db::upsert_task_credentials(
        &pool,
        &task.id,
        &task.protocol,
        "alice",
        "rotated-secret",
        None,
        None,
    )
    .await
    .expect("upsert credentials");

    // The row must hold ciphertext, not the plaintext secret.
    let raw: (String,) =
        sqlx::query_as("SELECT credentials_ciphertext FROM task_credentials WHERE task_id = ?")
            .bind(&task.id)
            .fetch_one(&pool)
            .await
            .expect("credential row");
    assert!(
        !raw.0.contains("rotated-secret"),
        "secret must be encrypted at rest"
    );

    let resolved = db::resolve_task_credentials(&pool, &task.id)
        .await
        .expect("resolve credentials")
        .expect("credentials present");
    assert_eq!(resolved.username, "alice");
    assert_eq!(resolved.password, "rotated-secret");
    assert_eq!(resolved.private_key_data, None);

    pool.close().await;
}
