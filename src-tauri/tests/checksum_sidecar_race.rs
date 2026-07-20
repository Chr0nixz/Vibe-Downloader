//! FUN-05: delayed sidecar checksum discovery after task completion.

mod common;

use sha2::{Digest, Sha256};
use tauri_app_lib::{
    commands::tasks::maybe_verify_completed_task_after_checksum_insert,
    db,
    models::{
        task::now_iso, ChecksumAlgorithm, HashVerificationStatus, TaskChecksumRecord, TaskStatus,
    },
};

fn sha256_hex(payload: &[u8]) -> String {
    Sha256::digest(payload)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test]
async fn fun05_delayed_sidecar_on_completed_task_verifies() {
    let pool = common::test_pool("fun05-sidecar").await;
    let paths = common::TestPaths::new("fun05-sidecar");
    let payload = b"fun05-sidecar-payload";
    std::fs::write(&paths.final_path, payload).expect("write completed file");

    let expected = sha256_hex(payload);
    let mut task = common::download_task(
        "fun05-sidecar",
        "https://example.com/file.bin".to_string(),
        "https",
        "file.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    task.status = TaskStatus::Completed;
    task.downloaded_bytes = payload.len() as i64;
    task.hash_status = HashVerificationStatus::NotRequested;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert completed task");

    // Simulate late sidecar insert after the worker already finished.
    let now = now_iso();
    db::insert_task_checksum_record(
        &pool,
        &TaskChecksumRecord {
            id: "sidecar-sha256".into(),
            task_id: task.id.clone(),
            file_id: None,
            algorithm: ChecksumAlgorithm::Sha256,
            expected_hash: expected.clone(),
            actual_hash: None,
            status: HashVerificationStatus::Pending,
            source_kind: "sidecar".into(),
            source_url: Some("https://example.com/file.bin.sha256".into()),
            source_label: Some("sha256".into()),
            is_primary: true,
            weak: false,
            error_message: None,
            discovered_at: Some(now.clone()),
            verified_at: None,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .expect("insert pending sidecar");

    maybe_verify_completed_task_after_checksum_insert(&pool, &task.id)
        .await
        .expect("post-complete verify");

    let checksums = db::list_task_checksum_records(&pool, &task.id)
        .await
        .expect("list checksums");
    assert_eq!(checksums.len(), 1);
    assert_eq!(checksums[0].status, HashVerificationStatus::Verified);
    assert_eq!(checksums[0].actual_hash.as_deref(), Some(expected.as_str()));

    let updated = db::get_task_record(&pool, &task.id)
        .await
        .expect("read task")
        .expect("task exists");
    assert_eq!(updated.hash_status, HashVerificationStatus::Verified);
}

#[tokio::test]
async fn fun05_delayed_sidecar_mismatch_marks_failed() {
    let pool = common::test_pool("fun05-sidecar-fail").await;
    let paths = common::TestPaths::new("fun05-sidecar-fail");
    let payload = b"fun05-mismatch";
    std::fs::write(&paths.final_path, payload).expect("write completed file");

    let mut task = common::download_task(
        "fun05-sidecar-fail",
        "https://example.com/mismatch.bin".to_string(),
        "https",
        "mismatch.bin",
        payload.len() as i64,
        &paths,
        false,
    );
    task.status = TaskStatus::Completed;
    task.downloaded_bytes = payload.len() as i64;
    task.hash_status = HashVerificationStatus::NotRequested;
    db::insert_task_record(&pool, &task)
        .await
        .expect("insert completed task");

    let now = now_iso();
    db::insert_task_checksum_record(
        &pool,
        &TaskChecksumRecord {
            id: "sidecar-bad".into(),
            task_id: task.id.clone(),
            file_id: None,
            algorithm: ChecksumAlgorithm::Sha256,
            expected_hash: "0".repeat(64),
            actual_hash: None,
            status: HashVerificationStatus::Pending,
            source_kind: "sidecar".into(),
            source_url: Some("https://example.com/mismatch.bin.sha256".into()),
            source_label: Some("sha256".into()),
            is_primary: true,
            weak: false,
            error_message: None,
            discovered_at: Some(now.clone()),
            verified_at: None,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .expect("insert pending sidecar");

    maybe_verify_completed_task_after_checksum_insert(&pool, &task.id)
        .await
        .expect("post-complete verify");

    let checksums = db::list_task_checksum_records(&pool, &task.id)
        .await
        .expect("list checksums");
    assert_eq!(checksums[0].status, HashVerificationStatus::Failed);

    let updated = db::get_task_record(&pool, &task.id)
        .await
        .expect("read task")
        .expect("task exists");
    assert_eq!(updated.hash_status, HashVerificationStatus::Failed);
}
