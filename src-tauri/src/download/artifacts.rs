//! Download artifact naming and lifecycle classification contract.
//!
//! Every temporary artifact the app creates is derivable from
//! `(task_id, final_path, save_dir, protocol)` — no owner registry is needed.
//! This module is the single naming authority for that derivation (lineage:
//! ARC-02 temp isolation, ARC-24 metalink part retention, ARC-38 staging
//! sweeps, ARC-39 DHT state isolation, ARC-43 BT runtime deletes, ARC-47
//! serial-path part cleanup) so the startup sweep and the Storage & Cleanup
//! Center share one definition of "orphan".
//!
//! Ownership rule, shared by every consumer: an artifact is reclaimable when
//! no task row claims it (orphan) or its only claimer is `Completed`; any
//! other claiming status keeps it, because retry/resume semantics depend on
//! the bytes staying on disk.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use sqlx::SqlitePool;
use uuid::Uuid;

use crate::db::ArtifactTaskRef;
use crate::models::{storage::ArtifactKind, TaskRecord, TaskStatus};

pub const TEMP_DOWNLOAD_SUFFIX: &str = ".vibe-downloading";
pub const STAGING_DIR_NAME: &str = ".vibe-staging";
/// Cross-volume publish leftover pattern from `file_ops::publish_across_volumes`:
/// `{final}.{token}.staging`. A crash between copy and rename leaks the copy.
pub const PUBLISH_STAGING_SUFFIX: &str = ".staging";
/// Metalink parallel workers write `{temp}.part-{N}` beside the temp file.
pub const METALINK_PART_INFIX: &str = ".part-";
/// ARC-39: per-task BT DHT persistence files in the system temp dir. The file
/// name hashes the session key, so there is no task mapping — age is the only
/// safe reclaim signal.
pub const DHT_STATE_PREFIX: &str = "vibe-dht-";
pub const DHT_STATE_SUFFIX: &str = ".json";
/// DHT state files are tiny; only surface them once they are clearly stale.
pub const DHT_STATE_STALE_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// Hard cap on directory entries examined per save dir during a scan. Keeps a
/// pathological library from turning the Storage Center into an unbounded
/// walk (PERF-16 lesson); the result reports `truncated` so the UI can say so.
pub const SCAN_MAX_ENTRIES_PER_DIR: usize = 20_000;

// ---------------------------------------------------------------------------
// Path derivation (single authority; task_file_planning re-exports these)
// ---------------------------------------------------------------------------

/// Per-task temp file path. Includes task UUID so concurrent same-name
/// downloads never share a temporary file.
pub fn task_temp_file_path(final_path: &Path, task_id: &str) -> PathBuf {
    PathBuf::from(format!(
        "{}.{task_id}{TEMP_DOWNLOAD_SUFFIX}",
        final_path.display()
    ))
}

/// Legacy temp path used before ARC-02 (`{final}.vibe-downloading`).
pub fn legacy_temp_file_path(final_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}{TEMP_DOWNLOAD_SUFFIX}", final_path.display()))
}

/// HLS/DASH staging directory isolated per task under the save directory.
pub fn task_staging_dir(save_dir: &Path, task_id: &str) -> PathBuf {
    save_dir.join(STAGING_DIR_NAME).join(task_id)
}

/// Temp path stored on the task row: staging dir for HLS, UUID temp file otherwise.
pub fn task_stored_temp_path(
    protocol: &str,
    save_dir: &Path,
    final_path: &Path,
    task_id: &str,
) -> PathBuf {
    if protocol == "hls" {
        task_staging_dir(save_dir, task_id)
    } else {
        task_temp_file_path(final_path, task_id)
    }
}

// ---------------------------------------------------------------------------
// Name classification (pure, no FS access)
// ---------------------------------------------------------------------------

/// What a temp-family file name decodes into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TempArtifactIdentity {
    /// `{final}.{task_id}.vibe-downloading` (ARC-02 layout).
    TempFile { final_stem: String, task_id: String },
    /// `{final}.vibe-downloading` (pre-ARC-02 stragglers, still resumable).
    LegacyTempFile { final_stem: String },
    /// `{final}.{token}.staging` cross-volume publish leftover.
    PublishStaging { final_stem: String, token: String },
    /// `{temp}.part-{index}` metalink worker part (ARC-24 layout).
    MetalinkPart { host_stem: String, index: u64 },
}

/// Classify a file name. Order matters: `.part-{n}` is tested first because a
/// metalink part name embeds the whole temp name (which itself ends in
/// `.vibe-downloading`).
///
/// The grammar is deliberately strict so user files can never be swept by
/// accident: a `.part-{n}` name only counts when the host is one of our temp
/// files, and a `.staging` leftover only counts when its token is a UUID —
/// `publish_token` always emits one (the embedded task UUID or a fresh v4).
pub fn classify_artifact_name(name: &str) -> Option<TempArtifactIdentity> {
    if let Some((host_stem, index)) = split_metalink_part(name) {
        if host_stem.ends_with(TEMP_DOWNLOAD_SUFFIX) {
            return Some(TempArtifactIdentity::MetalinkPart { host_stem, index });
        }
    }
    if let Some(rest) = name.strip_suffix(TEMP_DOWNLOAD_SUFFIX) {
        if let Some((final_stem, task_id)) = rest.rsplit_once('.') {
            if Uuid::parse_str(task_id).is_ok() {
                return Some(TempArtifactIdentity::TempFile {
                    final_stem: final_stem.to_string(),
                    task_id: task_id.to_string(),
                });
            }
        }
        return Some(TempArtifactIdentity::LegacyTempFile {
            final_stem: rest.to_string(),
        });
    }
    if let Some(rest) = name.strip_suffix(PUBLISH_STAGING_SUFFIX) {
        if let Some((final_stem, token)) = rest.rsplit_once('.') {
            if Uuid::parse_str(token).is_ok() {
                return Some(TempArtifactIdentity::PublishStaging {
                    final_stem: final_stem.to_string(),
                    token: token.to_string(),
                });
            }
        }
    }
    None
}

fn split_metalink_part(name: &str) -> Option<(String, u64)> {
    let index_start = name.rfind(METALINK_PART_INFIX)? + METALINK_PART_INFIX.len();
    let index = name[index_start..].parse::<u64>().ok()?;
    Some((
        name[..index_start - METALINK_PART_INFIX.len()].to_string(),
        index,
    ))
}

/// Extract the task UUID embedded in a temp file stem
/// (`{final}.{task_id}` → `task_id`), when present and valid.
fn decode_task_id_from_temp_stem(stem: &str) -> Option<&str> {
    let (_, task_id) = stem.rsplit_once('.')?;
    Uuid::parse_str(task_id).ok().map(|_| task_id)
}

/// Statuses whose artifacts must stay on disk: every non-completed status
/// keeps resume/retry semantics (HTTP temps, metalink parts, staging dirs).
pub fn status_keeps_artifacts(status: &str) -> bool {
    status != TaskStatus::Completed.as_str()
}

// ---------------------------------------------------------------------------
// Ownership resolution
// ---------------------------------------------------------------------------

/// Minimal task projection needed to resolve artifact ownership. The DB
/// projection lives in `db::ArtifactTaskRef`; this module consumes it
/// directly so scan, sweep, and cleanup share one type.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ownership {
    /// Safe to reclaim. `owner_task_id` is `Some` only when a `Completed`
    /// task still owns the bytes (leftover after publish).
    Reclaimable { owner_task_id: Option<String> },
    /// A live, non-completed task needs the artifact on disk.
    Keep { owner_task_id: String },
}

/// Resolve ownership of one classified artifact found under `dir`.
///
/// `dir`/`path` string comparisons use the exact same lossy form the DB
/// stores, matching the conventions in `unique_final_path_among`.
pub fn resolve_ownership(
    identity: &TempArtifactIdentity,
    dir: &Path,
    tasks: &[ArtifactTaskRef],
) -> Ownership {
    let dir_str = dir.to_string_lossy();
    let claim = |task: &ArtifactTaskRef| task.id.clone();
    let keeps = |task: &ArtifactTaskRef| status_keeps_artifacts(&task.status);
    let completed_owner = |task: &ArtifactTaskRef| Ownership::Reclaimable {
        owner_task_id: Some(claim(task)),
    };

    match identity {
        TempArtifactIdentity::TempFile { task_id, .. } => match lookup_id(tasks, task_id) {
            None => Ownership::Reclaimable {
                owner_task_id: None,
            },
            Some(task) if keeps(task) => Ownership::Keep {
                owner_task_id: claim(task),
            },
            Some(task) => completed_owner(task),
        },
        TempArtifactIdentity::LegacyTempFile { final_stem } => {
            // Exact temp_path match first: BT and non-UUID task ids record
            // temp paths that the name grammar cannot bind to an owner, and
            // a recorded path is the strongest ownership signal.
            let this_path = Path::new(dir_str.as_ref())
                .join(format!("{final_stem}{TEMP_DOWNLOAD_SUFFIX}"))
                .to_string_lossy()
                .to_string();
            if let Some(task) = tasks
                .iter()
                .find(|task| task.temp_path.as_deref() == Some(this_path.as_str()))
            {
                if keeps(task) {
                    return Ownership::Keep {
                        owner_task_id: claim(task),
                    };
                }
                return completed_owner(task);
            }
            let candidate_final = Path::new(dir_str.as_ref()).join(final_stem);
            claim_by_final_path(tasks, &candidate_final.to_string_lossy())
        }
        TempArtifactIdentity::PublishStaging { final_stem, token } => {
            let candidate_final = Path::new(dir_str.as_ref()).join(final_stem);
            let candidate_final = candidate_final.to_string_lossy();
            // The publish staging file is live while its owning task is still
            // publishing (downloading) or resumable afterwards.
            if let Some(task) = lookup_id(tasks, token) {
                if keeps(task) {
                    return Ownership::Keep {
                        owner_task_id: claim(task),
                    };
                }
                return completed_owner(task);
            }
            claim_by_final_path(tasks, &candidate_final)
        }
        TempArtifactIdentity::MetalinkPart { host_stem, .. } => {
            let host_path = Path::new(dir_str.as_ref()).join(host_stem);
            let host_str = host_path.to_string_lossy();
            // The part belongs to whatever live task owns the host temp file:
            // either by exact temp_path equality or by the UUID embedded in
            // the host stem.
            if let Some(task) = tasks
                .iter()
                .find(|task| task.temp_path.as_deref() == Some(host_str.as_ref()))
            {
                if keeps(task) {
                    return Ownership::Keep {
                        owner_task_id: claim(task),
                    };
                }
                return completed_owner(task);
            }
            if let Some(task_id) = decode_task_id_from_temp_stem(
                host_stem
                    .strip_suffix(TEMP_DOWNLOAD_SUFFIX)
                    .unwrap_or(host_stem),
            ) {
                return match lookup_id(tasks, task_id) {
                    None => Ownership::Reclaimable {
                        owner_task_id: None,
                    },
                    Some(task) if keeps(task) => Ownership::Keep {
                        owner_task_id: claim(task),
                    },
                    Some(task) => completed_owner(task),
                };
            }
            Ownership::Reclaimable {
                owner_task_id: None,
            }
        }
    }
}

fn lookup_id<'a>(tasks: &'a [ArtifactTaskRef], id: &str) -> Option<&'a ArtifactTaskRef> {
    tasks.iter().find(|task| task.id == id)
}

fn claim_by_final_path(tasks: &[ArtifactTaskRef], candidate_final: &str) -> Ownership {
    let mut completed = None;
    for task in tasks {
        let claims = task.final_path.as_deref() == Some(candidate_final);
        if !claims {
            continue;
        }
        if status_keeps_artifacts(&task.status) {
            return Ownership::Keep {
                owner_task_id: task.id.clone(),
            };
        }
        completed = Some(task);
    }
    match completed {
        Some(task) => Ownership::Reclaimable {
            owner_task_id: Some(task.id.clone()),
        },
        None => Ownership::Reclaimable {
            owner_task_id: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Per-task auxiliary artifacts (staging dirs + metalink parts)
// ---------------------------------------------------------------------------

/// Derive every auxiliary artifact of one task from its record: the HLS/DASH
/// staging directory and metalink `.part-N` siblings of its temp file(s).
///
/// Callers use this for task deletion, restart-from-beginning, and the
/// abandon-resume cleanup so all three share one enumeration instead of the
/// per-protocol special cases that leaked parts historically (ARC-38/47).
/// Deduplication is left to `delete_paths_off_runtime`.
pub async fn task_auxiliary_artifacts(
    task: &TaskRecord,
    file_temp_paths: &[String],
) -> Vec<PathBuf> {
    let mut artifacts = Vec::new();
    let save_dir = PathBuf::from(&task.save_dir);

    // DASH staging cannot be derived from temp/final paths (the recorded temp
    // is the remux output), so it is always derived from save_dir + task id.
    if matches!(task.protocol.as_str(), "hls" | "dash") {
        artifacts.push(task_staging_dir(&save_dir, &task.id));
    }

    if task.protocol == "metalink" {
        let mut temps: Vec<PathBuf> = file_temp_paths.iter().map(PathBuf::from).collect();
        if let Some(temp) = task.temp_path.as_deref().filter(|p| !p.trim().is_empty()) {
            temps.push(PathBuf::from(temp));
        }
        for temp in temps {
            let Some(parent) = temp.parent() else {
                continue;
            };
            let Some(temp_name) = temp.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let prefix = format!("{temp_name}{METALINK_PART_INFIX}");
            let Ok(mut entries) = tokio::fs::read_dir(parent).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&prefix) {
                    artifacts.push(parent.join(&name));
                }
            }
        }
    }

    artifacts
}

// ---------------------------------------------------------------------------
// Filesystem scan
// ---------------------------------------------------------------------------

/// One classified artifact found on disk.
#[derive(Debug, Clone)]
pub struct ArtifactScanEntry {
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub save_dir: String,
    pub file_name: String,
    pub bytes: u64,
    pub modified_at: Option<std::time::SystemTime>,
    pub ownership: Ownership,
}

/// Result of scanning one save dir.
#[derive(Debug, Default)]
pub struct DirScanOutcome {
    pub entries: Vec<ArtifactScanEntry>,
    pub truncated: bool,
}

/// Walk one save dir (bounded) and classify every temp-family artifact.
///
/// `.vibe-staging` is not descended into during the walk; its one-level
/// `{task_id}` children are reported as `StagingDir` entries instead. Final
/// user files never match the artifact name grammar, so the walk only stats
/// entries it classifies.
pub async fn scan_save_dir(save_dir: &Path, tasks: &[ArtifactTaskRef]) -> DirScanOutcome {
    let mut outcome = DirScanOutcome::default();
    let save_dir_str = save_dir.to_string_lossy().to_string();
    let mut stack = vec![save_dir.to_path_buf()];
    let mut visited = 0_usize;

    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(error) => {
                // Unreadable subtree: warn and keep scanning the rest rather
                // than aborting the whole inventory (startup sweep contract).
                tracing::warn!(dir = %dir.display(), error = %error, "artifact scan could not list directory");
                continue;
            }
        };
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(dir = %dir.display(), error = %error, "artifact scan directory read failed");
                    break;
                }
            };
            visited += 1;
            if visited > SCAN_MAX_ENTRIES_PER_DIR {
                outcome.truncated = true;
                return outcome;
            }
            let path = entry.path();
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            let name = entry.file_name().to_string_lossy().to_string();

            if is_dir {
                if dir == save_dir && name == STAGING_DIR_NAME {
                    collect_staging_dirs(&path, &save_dir_str, tasks, &mut outcome).await;
                } else {
                    stack.push(path);
                }
                continue;
            }

            let Some(identity) = classify_artifact_name(&name) else {
                continue;
            };
            let ownership = resolve_ownership(&identity, &dir, tasks);
            let (bytes, modified_at) = match entry.metadata().await {
                Ok(meta) => (meta.len(), meta.modified().ok()),
                Err(_) => (0, None),
            };
            outcome.entries.push(ArtifactScanEntry {
                path,
                kind: artifact_kind_for(&identity),
                save_dir: save_dir_str.clone(),
                file_name: name,
                bytes,
                modified_at,
                ownership,
            });
        }
    }
    outcome
}

/// DHT state files are reported only once stale — a fresh file may belong to
/// a running session and there is no owner mapping to consult.
pub async fn scan_stale_dht_states() -> Vec<ArtifactScanEntry> {
    let temp_dir = std::env::temp_dir();
    let mut entries = Vec::new();
    let Ok(mut reader) = tokio::fs::read_dir(&temp_dir).await else {
        return entries;
    };
    while let Ok(Some(entry)) = reader.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dht = name.starts_with(DHT_STATE_PREFIX) && name.ends_with(DHT_STATE_SUFFIX);
        if !is_dht {
            continue;
        }
        let Some(meta) = entry.metadata().await.ok() else {
            continue;
        };
        let modified_at = meta.modified().ok();
        let stale = modified_at
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age >= DHT_STATE_STALE_AGE);
        if !stale {
            continue;
        }
        entries.push(ArtifactScanEntry {
            path: entry.path(),
            kind: ArtifactKind::DhtState,
            save_dir: temp_dir.to_string_lossy().to_string(),
            file_name: name,
            bytes: meta.len(),
            modified_at,
            ownership: Ownership::Reclaimable {
                owner_task_id: None,
            },
        });
    }
    entries
}

async fn collect_staging_dirs(
    staging_root: &Path,
    save_dir: &str,
    tasks: &[ArtifactTaskRef],
    outcome: &mut DirScanOutcome,
) {
    let Ok(mut children) = tokio::fs::read_dir(staging_root).await else {
        return;
    };
    while let Ok(Some(child)) = children.next_entry().await {
        if !child.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let task_id = child.file_name().to_string_lossy().to_string();
        let ownership = match lookup_id(tasks, &task_id) {
            None => Ownership::Reclaimable {
                owner_task_id: None,
            },
            Some(task) if keeps_artifacts(&task.status) => Ownership::Keep {
                owner_task_id: task.id.clone(),
            },
            Some(task) => Ownership::Reclaimable {
                owner_task_id: Some(task.id.clone()),
            },
        };
        let (bytes, truncated) = dir_size_bounded(&child.path(), SCAN_MAX_ENTRIES_PER_DIR).await;
        if truncated {
            outcome.truncated = true;
        }
        outcome.entries.push(ArtifactScanEntry {
            path: child.path(),
            kind: ArtifactKind::StagingDir,
            save_dir: save_dir.to_string(),
            file_name: task_id,
            bytes,
            modified_at: child.metadata().await.ok().and_then(|m| m.modified().ok()),
            ownership,
        });
    }
}

fn keeps_artifacts(status: &str) -> bool {
    status_keeps_artifacts(status)
}

fn artifact_kind_for(identity: &TempArtifactIdentity) -> ArtifactKind {
    match identity {
        TempArtifactIdentity::TempFile { .. } => ArtifactKind::TempFile,
        TempArtifactIdentity::LegacyTempFile { .. } => ArtifactKind::LegacyTempFile,
        TempArtifactIdentity::PublishStaging { .. } => ArtifactKind::PublishStaging,
        TempArtifactIdentity::MetalinkPart { .. } => ArtifactKind::MetalinkPart,
    }
}

/// Bounded recursive size sum. `(0, false)` for unreadable paths — callers
/// treat scan failures as best-effort visibility, never as deletion criteria.
async fn dir_size_bounded(path: &Path, max_entries: usize) -> (u64, bool) {
    let mut total = 0_u64;
    let mut visited = 0_usize;
    let mut truncated = false;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            visited += 1;
            if visited > max_entries {
                truncated = true;
                return (total, truncated);
            }
            let Ok(meta) = entry.metadata().await else {
                continue;
            };
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    (total, truncated)
}

/// Best-effort byte size of one artifact path: file length, or bounded
/// recursive size for directories. `0` for unreadable paths — callers use
/// this for reporting only, never as a deletion criterion.
pub async fn artifact_bytes(path: &Path) -> u64 {
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.is_dir() => dir_size_bounded(path, SCAN_MAX_ENTRIES_PER_DIR).await.0,
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

// ---------------------------------------------------------------------------
// Sweep
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct SweepSummary {
    pub removed: usize,
    pub failed: usize,
    pub reclaimed_bytes: u64,
}

/// Sweep scope knobs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SweepOptions {
    /// Include stale DHT state files from the system temp dir. Production
    /// startup sweeps enable this; tests disable it so they never touch
    /// files outside their own temp base.
    pub include_dht: bool,
}

/// Startup sweep: remove every artifact classified reclaimable under the
/// known save dirs (ARC-38 generalization). Per-entry failures are warn-only
/// so one locked file (e.g. an antivirus scanner) cannot abort the whole
/// sweep — the previous behavior failed the entire remaining sweep on the
/// first error.
pub async fn sweep_orphan_artifacts(
    pool: &SqlitePool,
    extra_save_dirs: &[String],
    options: SweepOptions,
) -> Result<SweepSummary, String> {
    let task_refs = crate::db::list_artifact_task_refs(pool).await?;
    let mut save_dirs: Vec<String> = task_refs
        .iter()
        .map(|task| task.save_dir.clone())
        .chain(extra_save_dirs.iter().cloned())
        .collect();
    save_dirs.sort();
    save_dirs.dedup();

    let mut summary = SweepSummary::default();
    for save_dir in &save_dirs {
        let outcome = scan_save_dir(Path::new(save_dir), &task_refs).await;
        for entry in outcome.entries {
            if !matches!(entry.ownership, Ownership::Reclaimable { .. }) {
                continue;
            }
            remove_entry(&entry, &mut summary).await;
        }
    }
    if options.include_dht {
        for entry in scan_stale_dht_states().await {
            remove_entry(&entry, &mut summary).await;
        }
    }
    Ok(summary)
}

async fn remove_entry(entry: &ArtifactScanEntry, summary: &mut SweepSummary) {
    let is_dir = entry.kind == ArtifactKind::StagingDir;
    let result = if is_dir {
        tokio::fs::remove_dir_all(&entry.path).await
    } else {
        tokio::fs::remove_file(&entry.path).await
    };
    match result {
        Ok(()) => {
            summary.removed += 1;
            summary.reclaimed_bytes += entry.bytes;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Already gone — count as reclaimed success.
            summary.removed += 1;
            summary.reclaimed_bytes += entry.bytes;
        }
        Err(error) => {
            summary.failed += 1;
            tracing::warn!(
                path = %entry.path.display(),
                error = %error,
                "artifact sweep could not remove entry"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ref(
        id: &str,
        status: &str,
        temp: Option<String>,
        final_path: Option<String>,
    ) -> ArtifactTaskRef {
        ArtifactTaskRef {
            id: id.to_string(),
            save_dir: "/downloads".to_string(),
            status: status.to_string(),
            protocol: "https".to_string(),
            file_name: "file.bin".to_string(),
            temp_path: temp,
            final_path,
        }
    }

    #[test]
    fn classify_uuid_temp_file() {
        let id = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";
        let identity = classify_artifact_name(&format!("ubuntu.iso.{id}{TEMP_DOWNLOAD_SUFFIX}"))
            .expect("uuid temp");
        match identity {
            TempArtifactIdentity::TempFile {
                final_stem,
                task_id,
            } => {
                assert_eq!(final_stem, "ubuntu.iso");
                assert_eq!(task_id, id);
            }
            other => panic!("unexpected identity: {other:?}"),
        }
    }

    #[test]
    fn classify_temp_with_dotted_final_name() {
        // The final name may contain dots; the task UUID is the last segment.
        let id = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";
        let identity =
            classify_artifact_name(&format!("my.file.name.iso.{id}{TEMP_DOWNLOAD_SUFFIX}"))
                .expect("dotted temp");
        assert_eq!(
            identity,
            TempArtifactIdentity::TempFile {
                final_stem: "my.file.name.iso".to_string(),
                task_id: id.to_string(),
            }
        );
    }

    #[test]
    fn classify_legacy_temp_file() {
        let identity = classify_artifact_name("ubuntu.iso.vibe-downloading").expect("legacy temp");
        assert_eq!(
            identity,
            TempArtifactIdentity::LegacyTempFile {
                final_stem: "ubuntu.iso".to_string(),
            }
        );
    }

    #[test]
    fn classify_publish_staging_and_metalink_part() {
        let id = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";
        // publish_token only ever produces UUID tokens, so a leftover is
        // always `uuid`-shaped.
        let staging = classify_artifact_name(&format!("ubuntu.iso.{id}.staging")).expect("staging");
        assert_eq!(
            staging,
            TempArtifactIdentity::PublishStaging {
                final_stem: "ubuntu.iso".to_string(),
                token: id.to_string(),
            }
        );
        let part = classify_artifact_name("ubuntu.iso.vibe-downloading.part-3").expect("part");
        assert_eq!(
            part,
            TempArtifactIdentity::MetalinkPart {
                host_stem: "ubuntu.iso.vibe-downloading".to_string(),
                index: 3,
            }
        );
    }

    #[test]
    fn classify_ignores_user_files() {
        assert!(classify_artifact_name("ubuntu.iso").is_none());
        assert!(classify_artifact_name("movie.part2.mkv").is_none());
        assert!(classify_artifact_name("backup.staging.old").is_none());
        assert!(classify_artifact_name("notes.vibe-downloading.bak").is_none());
        // User files that merely look like artifacts must never classify —
        // otherwise the sweep would delete them as orphans.
        assert!(classify_artifact_name("notes.part-1").is_none());
        assert!(classify_artifact_name("release.part-12").is_none());
        assert!(classify_artifact_name("foo.bar.staging").is_none());
    }

    #[test]
    fn ownership_orphan_and_completed_reclaimable() {
        let id = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";
        let name = format!("file.bin.{id}{TEMP_DOWNLOAD_SUFFIX}");
        let identity = classify_artifact_name(&name).unwrap();
        let dir = Path::new("/downloads");

        // No task row → orphan.
        assert_eq!(
            resolve_ownership(&identity, dir, &[]),
            Ownership::Reclaimable {
                owner_task_id: None
            }
        );
        // Completed owner → reclaimable leftover.
        let completed = vec![temp_ref(id, "completed", None, None)];
        assert_eq!(
            resolve_ownership(&identity, dir, &completed),
            Ownership::Reclaimable {
                owner_task_id: Some(id.to_string()),
            }
        );
        // Paused owner → keep for resume.
        let paused = vec![temp_ref(id, "paused", None, None)];
        assert_eq!(
            resolve_ownership(&identity, dir, &paused),
            Ownership::Keep {
                owner_task_id: id.to_string(),
            }
        );
    }

    #[test]
    fn ownership_legacy_temp_matches_by_final_path() {
        let identity = classify_artifact_name("file.bin.vibe-downloading").unwrap();
        let dir = Path::new("/downloads");
        // Build the stored final path with Path::join so the comparison uses
        // the same platform separators as resolve_ownership itself.
        let final_path = dir.join("file.bin").to_string_lossy().to_string();
        let live = vec![temp_ref("task-1", "failed", None, Some(final_path.clone()))];
        assert_eq!(
            resolve_ownership(&identity, dir, &live),
            Ownership::Keep {
                owner_task_id: "task-1".to_string(),
            }
        );
        // Completed owner of the same final path → reclaimable.
        let completed = vec![temp_ref("task-1", "completed", None, Some(final_path))];
        assert_eq!(
            resolve_ownership(&identity, dir, &completed),
            Ownership::Reclaimable {
                owner_task_id: Some("task-1".to_string()),
            }
        );
    }

    #[test]
    fn ownership_metalink_part_follows_host_temp() {
        let id = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";
        let dir = Path::new("/downloads");
        let temp = dir
            .join(format!("file.bin.{id}{TEMP_DOWNLOAD_SUFFIX}"))
            .to_string_lossy()
            .to_string();
        let identity =
            classify_artifact_name(&format!("file.bin.{id}{TEMP_DOWNLOAD_SUFFIX}.part-1")).unwrap();
        let downloading = vec![temp_ref(id, "downloading", Some(temp), None)];
        assert_eq!(
            resolve_ownership(&identity, dir, &downloading),
            Ownership::Keep {
                owner_task_id: id.to_string(),
            }
        );
        // No matching task → orphan (the exact ARC-47 leak shape). The task
        // id is still decoded from the host stem when temp_path is absent.
        assert_eq!(
            resolve_ownership(&identity, dir, &[]),
            Ownership::Reclaimable {
                owner_task_id: None
            }
        );
        let resumable = vec![temp_ref(id, "paused", None, None)];
        assert_eq!(
            resolve_ownership(&identity, dir, &resumable),
            Ownership::Keep {
                owner_task_id: id.to_string(),
            }
        );
    }

    #[test]
    fn ownership_publish_staging_follows_final_or_token() {
        let id = "0b6fd7e0-2f7a-4d3f-9a1b-2c3d4e5f6a7b";
        let identity = classify_artifact_name(&format!("file.bin.{id}.staging")).unwrap();
        let dir = Path::new("/downloads");
        let final_path = dir.join("file.bin").to_string_lossy().to_string();
        let live = vec![temp_ref(id, "downloading", None, Some(final_path))];
        assert_eq!(
            resolve_ownership(&identity, dir, &live),
            Ownership::Keep {
                owner_task_id: id.to_string(),
            }
        );
        assert_eq!(
            resolve_ownership(&identity, dir, &[]),
            Ownership::Reclaimable {
                owner_task_id: None
            }
        );
    }

    #[test]
    fn status_keeps_artifacts_only_completed_releases() {
        for status in [
            "queued",
            "downloading",
            "paused",
            "failed",
            "retrying",
            "waiting_network",
            "needs_attention",
        ] {
            assert!(status_keeps_artifacts(status), "{status} must keep");
        }
        assert!(!status_keeps_artifacts("completed"));
    }
}
