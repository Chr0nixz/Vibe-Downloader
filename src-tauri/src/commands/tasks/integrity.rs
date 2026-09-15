//! Integrity Passport read path (feature proposal §2.4).
//!
//! Read-only assembly: one command aggregating the task record, checksum
//! records, event-derived milestones and a staging FS check. It must never
//! mutate state or delete artifacts — cleanup stays in the Storage Center.

use std::path::Path;

use tauri::State;

use crate::{
    db,
    download::artifacts::{task_staging_dir, PUBLISH_STAGING_SUFFIX},
    models::{
        integrity::{
            IntegrityPassport, PassportChecksum, PassportChecksumState, PassportStagingCleanup,
            RemoteValidatorKind,
        },
        HashVerificationStatus, TaskRecord, TaskStatus,
    },
    AppState,
};

use super::require_task;

/// Assemble the passport for one task. Public for integration tests; the
/// tauri command is a thin wrapper over this.
pub async fn build_integrity_passport(
    pool: &sqlx::SqlitePool,
    task_id: &str,
) -> Result<IntegrityPassport, String> {
    let task = require_task(pool, task_id).await?;

    let milestones = db::task_milestones(pool, task_id).await?;
    let resume_count = count_to_u32(db::count_task_resumes(pool, task_id).await?)?;
    let segment_retries = count_to_u32(db::sum_task_segment_retries(pool, task_id).await?)?;
    let checksums = collect_checksums(pool, &task).await?;
    let staging_cleanup = staging_cleanup_state(pool, &task).await?;

    // Remote validator evidence recorded by the probe/engines. These are
    // facts about what the download relied on, not a trust statement.
    let mut remote_validators = Vec::new();
    if non_empty(task.etag.as_deref()) {
        remote_validators.push(RemoteValidatorKind::Etag);
    }
    if non_empty(task.last_modified.as_deref()) {
        remote_validators.push(RemoteValidatorKind::LastModified);
    }
    if task.supports_resume {
        remote_validators.push(RemoteValidatorKind::Range);
    }

    Ok(IntegrityPassport {
        task_id: task.id.clone(),
        file_name: task.file_name.clone(),
        source_url: task.url.clone(),
        final_url: task.final_url.clone(),
        protocol: task.protocol.clone(),
        task_kind: task.task_kind,
        status: task.status,
        total_bytes: (task.total_size >= 0).then(|| task.total_size.to_string()),
        downloaded_bytes: Some(task.downloaded_bytes.max(0).to_string()),
        created_at: task.created_at.clone(),
        started_at: milestones.started_at,
        completed_at: milestones.completed_at,
        resume_count,
        segment_retries,
        supports_resume: task.supports_resume,
        remote_validators,
        checksum_state: derive_checksum_state(&checksums),
        checksums,
        staging_cleanup,
        final_path: task.final_path.clone(),
    })
}

/// Feature proposal §2.4: read-only integrity passport for one task.
#[tauri::command]
#[specta::specta]
pub async fn get_task_integrity_passport(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<IntegrityPassport, String> {
    build_integrity_passport(&state.pool, &task_id).await
}

fn non_empty(value: Option<&str>) -> bool {
    value.is_some_and(|v| !v.trim().is_empty())
}

fn count_to_u32(value: i64) -> Result<u32, String> {
    u32::try_from(value.max(0)).map_err(|_| "integrity_overflow".to_string())
}

/// Task-level checksums (`file_id IS NULL`) plus the legacy task-row columns
/// as a fallback when no structured rows exist. Per-file checksums are out of
/// scope for the task-level passport (proposal §2.4 v1).
async fn collect_checksums(
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
) -> Result<Vec<PassportChecksum>, String> {
    let records = db::list_task_checksum_records(pool, &task.id).await?;
    let mut checksums: Vec<PassportChecksum> = records
        .iter()
        .filter(|record| record.file_id.is_none())
        .map(|record| PassportChecksum {
            algorithm: record.algorithm.as_str().to_string(),
            status: record.status,
            actual_hash: record.actual_hash.clone(),
            verified_at: record.verified_at.clone(),
            is_primary: record.is_primary,
            weak: record.weak,
            source_kind: record.source_kind.clone(),
            error_message: record.error_message.clone(),
        })
        .collect();

    if checksums.is_empty() && non_empty(task.expected_hash_sha256.as_deref()) {
        // Legacy single-column hash path (manual SHA-256 input predating the
        // task_checksums table).
        checksums.push(PassportChecksum {
            algorithm: "sha256".to_string(),
            status: task.hash_status,
            actual_hash: task.actual_hash_sha256.clone(),
            verified_at: task.hash_verified_at.clone(),
            is_primary: true,
            weak: false,
            source_kind: "manual".to_string(),
            error_message: task.hash_error.clone(),
        });
    }
    Ok(checksums)
}

/// Precedence: any verified result wins (that is the trust claim), then the
/// latest failure, then a still-pending run. An empty list — or a list with
/// only `not_requested` entries — must read as "no checksum provided".
fn derive_checksum_state(checksums: &[PassportChecksum]) -> PassportChecksumState {
    if checksums
        .iter()
        .any(|c| c.status == HashVerificationStatus::Verified)
    {
        PassportChecksumState::Verified
    } else if checksums
        .iter()
        .any(|c| c.status == HashVerificationStatus::Failed)
    {
        PassportChecksumState::Failed
    } else if checksums
        .iter()
        .any(|c| c.status == HashVerificationStatus::Pending)
    {
        PassportChecksumState::Pending
    } else {
        PassportChecksumState::NotProvided
    }
}

/// Staging-cleanup proof via read-only FS checks. A completed task claims its
/// outputs are published; the passport verifies that claim instead of
/// trusting the status column (the file may have been moved or deleted).
async fn staging_cleanup_state(
    pool: &sqlx::SqlitePool,
    task: &TaskRecord,
) -> Result<PassportStagingCleanup, String> {
    if !matches!(task.status, TaskStatus::Completed) {
        return Ok(PassportStagingCleanup::NotApplicable);
    }

    // Output paths: the single-file path plus every selected task_file row
    // (multi-file manifests keep per-file final_path only). Duplicates from
    // overlapping rows are harmless — existence is idempotent.
    let mut outputs: Vec<String> = Vec::new();
    if let Some(final_path) = task.final_path.as_deref() {
        outputs.push(final_path.to_string());
    }
    for file in db::list_task_file_records(pool, &task.id)
        .await?
        .iter()
        .filter(|file| file.selected)
    {
        if let Some(final_path) = file.final_path.as_deref() {
            outputs.push(final_path.to_string());
        }
    }

    // Residue: the task temp file, its staging dir, or a `<output>.staging`
    // sibling left behind by a failed publish rename.
    let staging_dir = task_staging_dir(Path::new(&task.save_dir), &task.id);
    let residue = path_exists(task.temp_path.as_deref())
        || staging_dir.exists()
        || outputs
            .iter()
            .any(|output| Path::new(&format!("{output}{PUBLISH_STAGING_SUFFIX}")).exists());

    if outputs.is_empty() {
        // No known output path to verify — report residue honestly, but
        // never claim "complete" without disk evidence.
        return Ok(if residue {
            PassportStagingCleanup::Incomplete
        } else {
            PassportStagingCleanup::NotApplicable
        });
    }
    if outputs.iter().any(|output| !Path::new(output).exists()) {
        return Ok(PassportStagingCleanup::MissingOutput);
    }
    Ok(if residue {
        PassportStagingCleanup::Incomplete
    } else {
        PassportStagingCleanup::Complete
    })
}

fn path_exists(path: Option<&str>) -> bool {
    path.is_some_and(|p| Path::new(p).exists())
}
