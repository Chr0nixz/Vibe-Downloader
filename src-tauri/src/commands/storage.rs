//! Storage & Cleanup Center commands (feature proposal §3.2): artifact
//! inventory scan, categorized cleanup, per-task abandon-resume cleanup, and
//! sweep records.
//!
//! Contract invariants:
//! - The backend owns every path. The frontend receives opaque item ids (the
//!   absolute path string used only as an echo token) and never builds or
//!   joins paths itself.
//! - Cleanup re-scans immediately before deleting, so a stale scan can never
//!   delete something that became live in between: new tasks cannot claim an
//!   orphan temp's name (`unique_final_path_among` blocks any sibling temp)
//!   and live owners classify as `Keep`, never as reclaimable.
//! - Per-item outcomes are reported individually so partial success is never
//!   displayed as full success.

use std::{
    collections::HashMap,
    path::Path,
    time::{Instant, SystemTime},
};

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    commands::tasks::{delete_paths_off_runtime, FileDeleteRequest},
    db,
    download::artifacts::{self, Ownership},
    events::{emit_storage_cleanup_progress, StorageCleanupProgressPayload},
    models::storage::{
        ArtifactKind, ArtifactReason, CleanupItemOutcome, CleanupMode, CleanupOutcome,
        SaveDirOverview, StorageArtifactItem, StorageCleanupResult, StorageScanResult,
        StorageSweepRecord,
    },
    models::AppErrorPayload,
    AppState,
};

/// Deletion chunk size between progress emissions.
const DELETE_CHUNK_SIZE: usize = 16;
const PROGRESS_EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

fn storage_error(code: &str, message: impl Into<String>) -> String {
    AppErrorPayload::new(code, message, false, vec![]).command_error()
}

#[tauri::command]
#[specta::specta]
pub async fn scan_storage(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StorageScanResult, String> {
    let default_save_dir = crate::commands::settings::default_download_dir(&app)?;
    scan_storage_impl(&state.pool, &default_save_dir).await
}

#[tauri::command]
#[specta::specta]
pub async fn clean_storage_artifacts(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: CleanupMode,
    item_ids: Option<Vec<String>>,
) -> Result<StorageCleanupResult, String> {
    let request_id = Uuid::new_v4().to_string();
    let sweep_started = chrono::Utc::now().to_rfc3339();
    let default_save_dir = crate::commands::settings::default_download_dir(&app)?;
    // Fresh scan: this is the re-validation step. Nothing is deleted from a
    // cached inventory.
    let scan = scan_storage_impl(&state.pool, &default_save_dir).await?;

    let selected: Option<Vec<String>> = match mode {
        CleanupMode::Selected => Some(
            item_ids.ok_or_else(|| storage_error("storage_cleanup_failed", "Missing item ids"))?,
        ),
        _ => None,
    };

    let candidates: Vec<&StorageArtifactItem> = scan
        .items
        .iter()
        .filter(|item| is_mode_candidate(mode, item, selected.as_deref()))
        .collect();
    let bytes_by_path: HashMap<&str, u64> = candidates
        .iter()
        .map(|item| (item.id.as_str(), item.bytes.parse::<u64>().unwrap_or(0)))
        .collect();

    // Items explicitly picked but not reclaimable are reported as skipped so
    // a stale selection cannot silently delete kept (resumable) bytes.
    let mut skipped_ids: Vec<String> = Vec::new();
    if mode == CleanupMode::Selected {
        if let Some(ids) = &selected {
            for id in ids {
                if !candidates.iter().any(|item| &item.id == id) {
                    skipped_ids.push(id.clone());
                }
            }
        }
    }

    let requests: Vec<FileDeleteRequest> = candidates
        .iter()
        .map(|item| FileDeleteRequest {
            path: item.id.clone(),
            use_trash: false,
        })
        .collect();
    // Scan-order deletion keeps progress counts deterministic in tests.
    let total = requests.len() as u32;

    let mut progress = CleanupProgress::new(app.clone(), request_id.clone(), total);
    progress.emit_if_due(true, 0, 0, 0, 0);

    let mut outcomes: Vec<CleanupItemOutcome> = Vec::new();
    let mut removed = 0_u32;
    let mut failed = 0_u32;
    let mut reclaimed_bytes = 0_u64;
    for chunk in requests
        .chunks(DELETE_CHUNK_SIZE)
        .map(<[FileDeleteRequest]>::to_vec)
    {
        let results = delete_paths_off_runtime(chunk).await;
        for outcome in results {
            let path_bytes = bytes_by_path
                .get(outcome.path.as_str())
                .copied()
                .unwrap_or(0);
            match &outcome.result {
                Ok(()) => {
                    removed += 1;
                    reclaimed_bytes += path_bytes;
                    outcomes.push(CleanupItemOutcome {
                        item_id: outcome.path,
                        outcome: CleanupOutcome::Removed,
                        bytes: path_bytes.to_string(),
                        error_code: None,
                    });
                }
                Err(error) => {
                    failed += 1;
                    let failed_path = outcome.path.clone();
                    outcomes.push(CleanupItemOutcome {
                        item_id: outcome.path,
                        outcome: CleanupOutcome::Failed,
                        bytes: "0".to_string(),
                        error_code: Some("storage_cleanup_failed".to_string()),
                    });
                    tracing::warn!(path = %failed_path, error, "storage cleanup delete failed");
                }
            }
        }
        progress.emit_if_due(false, removed + failed, removed, failed, reclaimed_bytes);
    }

    for id in &skipped_ids {
        outcomes.push(CleanupItemOutcome {
            item_id: id.clone(),
            outcome: CleanupOutcome::Skipped,
            bytes: "0".to_string(),
            error_code: None,
        });
    }

    let mode_label = sweep_mode_label(mode);
    record_sweep(
        &state.pool,
        mode_label,
        &sweep_started,
        removed,
        failed,
        reclaimed_bytes,
        &outcomes,
    )
    .await;

    Ok(StorageCleanupResult {
        request_id,
        mode,
        removed_count: removed,
        skipped_count: skipped_ids.len() as u32,
        failed_count: failed,
        reclaimed_bytes: reclaimed_bytes.to_string(),
        outcomes,
        resume_discarded: false,
    })
}

/// Abandon-resume cleanup: remove every artifact of one paused/failed task.
/// Refuses any other status — a task that is queued/downloading/retrying owns
/// live bytes, and a completed task has nothing left to clean.
#[tauri::command]
#[specta::specta]
pub async fn cleanup_task_temp_files(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<StorageCleanupResult, String> {
    // R-2.3: serialize against start/pause/cancel on the same task.
    let _guard = state.task_runtime_locks.lock(&task_id).await;
    let task = db::get_task_record(&state.pool, &task_id)
        .await?
        .ok_or_else(|| storage_error("storage_task_busy", "Task not found"))?;
    if !matches!(
        task.status,
        crate::models::TaskStatus::Paused
            | crate::models::TaskStatus::Failed
            | crate::models::TaskStatus::WaitingNetwork
    ) {
        return Err(storage_error(
            "storage_task_busy",
            format!(
                "Task is {} and cannot be cleaned up right now.",
                task.status.as_str()
            ),
        ));
    }

    let request_id = Uuid::new_v4().to_string();
    let sweep_started = chrono::Utc::now().to_rfc3339();
    let file_temps: Vec<String> = db::list_task_file_records(&state.pool, &task_id)
        .await?
        .iter()
        .filter_map(|file| file.temp_path.clone())
        .collect();
    let mut paths: Vec<String> = file_temps;
    if let Some(temp) = task.temp_path.as_deref().filter(|p| !p.trim().is_empty()) {
        paths.push(temp.to_string());
    }
    for artifact in artifacts::task_auxiliary_artifacts(&task, &paths).await {
        paths.push(artifact.to_string_lossy().to_string());
    }

    // Measure before deleting so the sweep record and result report the real
    // reclaimed amount (staging dirs get the bounded recursive size).
    let mut path_bytes: HashMap<String, u64> = HashMap::new();
    for path in &paths {
        path_bytes.insert(
            path.clone(),
            artifacts::artifact_bytes(Path::new(path)).await,
        );
    }
    let requests: Vec<FileDeleteRequest> = paths
        .into_iter()
        .map(|path| FileDeleteRequest {
            path,
            use_trash: false,
        })
        .collect();
    let total = requests.len() as u32;
    let mut progress = CleanupProgress::new(app.clone(), request_id.clone(), total);
    progress.emit_if_due(true, 0, 0, 0, 0);

    let mut outcomes: Vec<CleanupItemOutcome> = Vec::new();
    let mut removed = 0_u32;
    let mut failed = 0_u32;
    let mut reclaimed_bytes = 0_u64;
    for chunk in requests
        .chunks(DELETE_CHUNK_SIZE)
        .map(<[FileDeleteRequest]>::to_vec)
    {
        let results = delete_paths_off_runtime(chunk).await;
        for outcome in results {
            let path_bytes = path_bytes.get(&outcome.path).copied().unwrap_or(0);
            match &outcome.result {
                Ok(()) => {
                    removed += 1;
                    reclaimed_bytes += path_bytes;
                }
                Err(_) => failed += 1,
            }
            outcomes.push(CleanupItemOutcome {
                item_id: outcome.path,
                outcome: if outcome.result.is_ok() {
                    CleanupOutcome::Removed
                } else {
                    CleanupOutcome::Failed
                },
                bytes: path_bytes.to_string(),
                error_code: outcome
                    .result
                    .is_err()
                    .then_some("storage_cleanup_failed".to_string()),
            });
        }
        progress.emit_if_due(false, removed + failed, removed, failed, reclaimed_bytes);
    }

    record_sweep(
        &state.pool,
        "task_abandon",
        &sweep_started,
        removed,
        failed,
        reclaimed_bytes,
        &outcomes,
    )
    .await;

    Ok(StorageCleanupResult {
        request_id,
        mode: CleanupMode::Selected,
        removed_count: removed,
        skipped_count: 0,
        failed_count: failed,
        reclaimed_bytes: reclaimed_bytes.to_string(),
        outcomes,
        resume_discarded: true,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn get_last_storage_sweep(
    state: State<'_, AppState>,
) -> Result<Option<StorageSweepRecord>, String> {
    db::latest_sweep_record(&state.pool).await
}

// ---------------------------------------------------------------------------
// Scan implementation
// ---------------------------------------------------------------------------

/// Statuses whose artifacts are surfaced in the per-task view. Actively
/// running tasks are not listed (their rows are visible in the task list);
/// only attention-worthy owners are.
fn owner_is_listable(status: &str) -> bool {
    matches!(
        status,
        "paused" | "failed" | "waiting_network" | "needs_attention"
    )
}

async fn scan_storage_impl(
    pool: &sqlx::SqlitePool,
    default_save_dir: &str,
) -> Result<StorageScanResult, String> {
    let task_refs = db::list_artifact_task_refs(pool)
        .await
        .map_err(|e| storage_error("storage_scan_failed", e))?;
    let avg_sizes = db::completed_avg_task_size_by_save_dir(pool)
        .await
        .map_err(|e| storage_error("storage_scan_failed", e))?;
    let owner_by_id: HashMap<&str, &db::ArtifactTaskRef> = task_refs
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();

    let mut save_dirs: Vec<String> = task_refs
        .iter()
        .map(|task| task.save_dir.clone())
        .chain(std::iter::once(default_save_dir.to_string()))
        .collect();
    save_dirs.sort();
    save_dirs.dedup();

    let mut dirs: Vec<SaveDirOverview> = Vec::new();
    let mut items: Vec<StorageArtifactItem> = Vec::new();

    for save_dir in &save_dirs {
        let outcome = artifacts::scan_save_dir(Path::new(save_dir), &task_refs).await;
        let mut reclaimable_bytes = 0_u64;
        let mut resumable_bytes = 0_u64;
        for entry in outcome.entries {
            match &entry.ownership {
                Ownership::Reclaimable { .. } => reclaimable_bytes += entry.bytes,
                Ownership::Keep { .. } => resumable_bytes += entry.bytes,
            }
            if let Some(item) = item_from_entry(&entry, &owner_by_id) {
                items.push(item);
            }
        }
        dirs.push(save_dir_overview(
            save_dir,
            reclaimable_bytes,
            resumable_bytes,
            avg_sizes
                .iter()
                .find(|(dir, _)| dir == save_dir)
                .map(|(_, avg)| *avg),
            outcome.truncated,
        ));
    }

    // Stale DHT states are system-temp artifacts with no owning task; they
    // are listed as reclaimable but do not belong to any save dir overview.
    for entry in artifacts::scan_stale_dht_states().await {
        if let Some(item) = item_from_entry(&entry, &owner_by_id) {
            items.push(item);
        }
    }

    Ok(StorageScanResult {
        scan_id: Uuid::new_v4().to_string(),
        scanned_at: chrono::Utc::now().to_rfc3339(),
        dirs,
        items,
    })
}

fn item_from_entry(
    entry: &artifacts::ArtifactScanEntry,
    owner_by_id: &HashMap<&str, &db::ArtifactTaskRef>,
) -> Option<StorageArtifactItem> {
    let (reclaimable, reason, owner_task_id) = match &entry.ownership {
        Ownership::Reclaimable { owner_task_id } => (
            true,
            if entry.kind == ArtifactKind::DhtState {
                ArtifactReason::DhtStale
            } else if owner_task_id.is_some() {
                ArtifactReason::OwnerCompleted
            } else {
                ArtifactReason::NoOwner
            },
            owner_task_id.clone(),
        ),
        Ownership::Keep { owner_task_id } => {
            // Running tasks are noise in this view; their rows live in the
            // task list. Only attention-worthy owners are listed.
            let listable = owner_by_id
                .get(owner_task_id.as_str())
                .is_some_and(|task| owner_is_listable(&task.status));
            if !listable {
                return None;
            }
            (
                false,
                ArtifactReason::OwnerResumable,
                Some(owner_task_id.clone()),
            )
        }
    };
    let (task_file_name, owner_protocol) = owner_task_id
        .as_deref()
        .and_then(|id| owner_by_id.get(id))
        .map(|task| (Some(task.file_name.clone()), Some(task.protocol.clone())))
        .unwrap_or((None, None));

    Some(StorageArtifactItem {
        id: entry.path.to_string_lossy().to_string(),
        kind: entry.kind,
        save_dir: entry.save_dir.clone(),
        file_name: entry.file_name.clone(),
        bytes: entry.bytes.to_string(),
        modified_at: entry.modified_at.map(system_time_to_rfc3339),
        reclaimable,
        reason,
        owner_task_id,
        task_file_name,
        owner_protocol,
    })
}

fn save_dir_overview(
    save_dir: &str,
    reclaimable_bytes: u64,
    resumable_bytes: u64,
    avg_completed_size: Option<i64>,
    truncated: bool,
) -> SaveDirOverview {
    // Walk up to the first existing ancestor so a not-yet-created save dir
    // still yields a meaningful volume reading (same rule as query_disk_space).
    let mut probe = Path::new(save_dir);
    while !probe.exists() {
        match probe.parent() {
            Some(parent) => probe = parent,
            None => break,
        }
    }
    let (total, available) = if probe.exists() {
        match (fs4::free_space(probe), fs4::available_space(probe)) {
            (Ok(total), Ok(available)) => (total, available),
            (..) => (0, 0),
        }
    } else {
        (0, 0)
    };
    let estimated = avg_completed_size
        .filter(|avg| *avg > 0 && available > 0)
        .map(|avg| (available / avg.max(1) as u64).to_string());

    SaveDirOverview {
        path: save_dir.to_string(),
        total_bytes: total.to_string(),
        available_bytes: available.to_string(),
        reclaimable_bytes: reclaimable_bytes.to_string(),
        resumable_bytes: resumable_bytes.to_string(),
        estimated_completable_tasks: estimated,
        truncated,
    }
}

fn system_time_to_rfc3339(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339()
}

// ---------------------------------------------------------------------------
// Cleanup helpers
// ---------------------------------------------------------------------------

struct CleanupProgress {
    app: AppHandle,
    request_id: String,
    total: u32,
    last_emit: Option<Instant>,
}

impl CleanupProgress {
    fn new(app: AppHandle, request_id: String, total: u32) -> Self {
        Self {
            app,
            request_id,
            total,
            last_emit: None,
        }
    }

    fn emit_if_due(
        &mut self,
        force: bool,
        processed: u32,
        removed: u32,
        failed: u32,
        reclaimed: u64,
    ) {
        let due = self
            .last_emit
            .is_none_or(|last| last.elapsed() >= PROGRESS_EMIT_INTERVAL);
        if !(force || due) {
            return;
        }
        self.last_emit = Some(Instant::now());
        emit_storage_cleanup_progress(
            &self.app,
            &StorageCleanupProgressPayload {
                request_id: self.request_id.clone(),
                processed,
                total: self.total,
                removed,
                failed,
                reclaimed_bytes: reclaimed.to_string(),
            },
        );
    }
}

/// Decide whether one freshly-scanned item is targeted by a cleanup mode.
///
/// Selection never widens the reclaimable gate: an id pointing at a resumable
/// (`Keep`) artifact — stale selection, or a request crafted against the scan —
/// is skipped, never deleted.
fn is_mode_candidate(
    mode: CleanupMode,
    item: &StorageArtifactItem,
    selected: Option<&[String]>,
) -> bool {
    match mode {
        CleanupMode::Orphans => item.reclaimable && item.reason == ArtifactReason::NoOwner,
        CleanupMode::CompletedLeftovers => {
            item.reclaimable && item.reason == ArtifactReason::OwnerCompleted
        }
        CleanupMode::AllReclaimable => item.reclaimable,
        CleanupMode::Selected => {
            item.reclaimable && selected.is_some_and(|ids| ids.iter().any(|id| id == &item.id))
        }
    }
}

fn sweep_mode_label(mode: CleanupMode) -> &'static str {
    match mode {
        CleanupMode::Orphans => "manual_orphans",
        CleanupMode::CompletedLeftovers => "manual_completed",
        CleanupMode::AllReclaimable => "manual_all",
        CleanupMode::Selected => "manual_selected",
    }
}

#[allow(clippy::too_many_arguments)]
async fn record_sweep(
    pool: &sqlx::SqlitePool,
    mode: &str,
    started_at: &str,
    removed: u32,
    failed: u32,
    reclaimed_bytes: u64,
    outcomes: &[CleanupItemOutcome],
) {
    let record = StorageSweepRecord {
        id: Uuid::new_v4().to_string(),
        started_at: started_at.to_string(),
        finished_at: chrono::Utc::now().to_rfc3339(),
        mode: mode.to_string(),
        removed_count: removed,
        failed_count: failed,
        reclaimed_bytes: reclaimed_bytes.to_string(),
    };
    // Failures go into the details blob so they stay out of the localized
    // record but remain available for diagnostics.
    let failures: Vec<&CleanupItemOutcome> = outcomes
        .iter()
        .filter(|outcome| outcome.outcome == CleanupOutcome::Failed)
        .collect();
    let details = serde_json::to_string(&failures).unwrap_or_else(|_| "{}".to_string());
    if let Err(error) = db::insert_sweep_record(pool, &record, &details).await {
        tracing::warn!(error = %error, "could not persist sweep record");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::storage::ArtifactKind;

    fn item(reclaimable: bool, reason: ArtifactReason) -> StorageArtifactItem {
        StorageArtifactItem {
            id: "/downloads/file.bin.task.vibe-downloading".to_string(),
            kind: ArtifactKind::TempFile,
            save_dir: "/downloads".to_string(),
            file_name: "file.bin.task.vibe-downloading".to_string(),
            bytes: "100".to_string(),
            modified_at: None,
            reclaimable,
            reason,
            owner_task_id: None,
            task_file_name: None,
            owner_protocol: None,
        }
    }

    #[test]
    fn aggregate_modes_follow_the_reclaimable_gate() {
        let orphan = item(true, ArtifactReason::NoOwner);
        let completed = item(true, ArtifactReason::OwnerCompleted);
        let resumable = item(false, ArtifactReason::OwnerResumable);

        assert!(is_mode_candidate(CleanupMode::Orphans, &orphan, None));
        assert!(!is_mode_candidate(CleanupMode::Orphans, &completed, None));
        assert!(!is_mode_candidate(CleanupMode::Orphans, &resumable, None));

        assert!(is_mode_candidate(
            CleanupMode::CompletedLeftovers,
            &completed,
            None
        ));
        assert!(!is_mode_candidate(
            CleanupMode::CompletedLeftovers,
            &orphan,
            None
        ));

        assert!(is_mode_candidate(
            CleanupMode::AllReclaimable,
            &orphan,
            None
        ));
        assert!(!is_mode_candidate(
            CleanupMode::AllReclaimable,
            &resumable,
            None
        ));
    }

    #[test]
    fn selected_mode_never_targets_resumable_or_unknown_items() {
        let reclaimable = item(true, ArtifactReason::NoOwner);
        let mut other = item(true, ArtifactReason::NoOwner);
        other.id = "/downloads/other.vibe-downloading".to_string();
        let resumable = item(false, ArtifactReason::OwnerResumable);

        let ids = vec![reclaimable.id.clone(), resumable.id.clone()];
        let selected = Some(ids.as_slice());

        // Reclaimable + selected → deleted.
        assert!(is_mode_candidate(
            CleanupMode::Selected,
            &reclaimable,
            selected
        ));
        // Selected but not reclaimable → skipped, never deleted (the command
        // contract: a stale selection cannot silently delete resumable bytes).
        assert!(!is_mode_candidate(
            CleanupMode::Selected,
            &resumable,
            selected
        ));
        // Reclaimable but not selected → untouched.
        assert!(!is_mode_candidate(CleanupMode::Selected, &other, selected));
        // Selected mode without an id list matches nothing.
        assert!(!is_mode_candidate(
            CleanupMode::Selected,
            &reclaimable,
            None
        ));
    }
}
