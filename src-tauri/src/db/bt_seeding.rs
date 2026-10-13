//! B6: cumulative, restart-safe seeding policy and checkpoints.

use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone)]
pub struct BtSeedingStatus {
    pub enabled: bool,
    pub manual_stop: bool,
    pub started_at: Option<String>,
    pub uploaded_bytes: i64,
    pub elapsed_seconds: i64,
    pub stop_reason: Option<String>,
    pub ratio_limit: Option<f64>,
    pub time_limit_seconds: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct BtSeedingState {
    pub info_hash: String,
    pub torrent_blob: Option<Vec<u8>>,
    pub enabled: bool,
    pub manual_stop: bool,
    pub started_at: Option<String>,
    pub uploaded_bytes: i64,
    pub elapsed_seconds: i64,
    pub stop_reason: Option<String>,
    pub ratio_limit: Option<f64>,
    pub time_limit_seconds: Option<i64>,
}

pub async fn bt_seeding_state(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<BtSeedingState>, String> {
    let row = sqlx::query("SELECT info_hash, torrent_blob, seeding_enabled, seed_manual_stop, seeding_started_at, seed_uploaded_bytes, seed_elapsed_seconds, seed_stop_reason, seed_ratio_limit, seed_time_limit_seconds FROM torrent_tasks WHERE task_id = ?").bind(task_id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    Ok(row.map(|row| BtSeedingState {
        info_hash: row.get("info_hash"),
        torrent_blob: row.get("torrent_blob"),
        enabled: row.get::<i64, _>("seeding_enabled") != 0,
        manual_stop: row.get::<i64, _>("seed_manual_stop") != 0,
        started_at: row.get("seeding_started_at"),
        uploaded_bytes: row.get::<i64, _>("seed_uploaded_bytes").max(0),
        elapsed_seconds: row.get::<i64, _>("seed_elapsed_seconds").max(0),
        stop_reason: row.get("seed_stop_reason"),
        ratio_limit: row.get("seed_ratio_limit"),
        time_limit_seconds: row.get("seed_time_limit_seconds"),
    }))
}

/// Poll policy and checkpoints without copying the cached metainfo blob.
pub async fn bt_seeding_status(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<BtSeedingStatus>, String> {
    let row = sqlx::query("SELECT seeding_enabled, seed_manual_stop, seeding_started_at, seed_uploaded_bytes, seed_elapsed_seconds, seed_stop_reason, seed_ratio_limit, seed_time_limit_seconds FROM torrent_tasks WHERE task_id = ?").bind(task_id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    Ok(row.map(|row| BtSeedingStatus {
        enabled: row.get::<i64, _>("seeding_enabled") != 0,
        manual_stop: row.get::<i64, _>("seed_manual_stop") != 0,
        started_at: row.get("seeding_started_at"),
        uploaded_bytes: row.get::<i64, _>("seed_uploaded_bytes").max(0),
        elapsed_seconds: row.get::<i64, _>("seed_elapsed_seconds").max(0),
        stop_reason: row.get("seed_stop_reason"),
        ratio_limit: row.get("seed_ratio_limit"),
        time_limit_seconds: row.get("seed_time_limit_seconds"),
    }))
}

pub async fn begin_bt_seeding(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("UPDATE torrent_tasks SET seeding_started_at = COALESCE(seeding_started_at, ?), seed_stop_reason = NULL, seed_manual_stop = 0, seeding_enabled = 1, updated_at = ? WHERE task_id = ?")
        .bind(crate::models::task::now_iso()).bind(crate::models::task::now_iso()).bind(task_id).execute(pool).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn checkpoint_bt_seeding(
    pool: &SqlitePool,
    task_id: &str,
    uploaded: i64,
    elapsed: i64,
    reason: Option<&str>,
) -> Result<(), String> {
    let mut tx = crate::db::begin_immediate(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE torrent_tasks SET seed_uploaded_bytes = MAX(seed_uploaded_bytes, ?), seed_elapsed_seconds = MAX(seed_elapsed_seconds, ?), seed_stop_reason = ?, seed_manual_stop = CASE WHEN ? = 'manual' THEN 1 ELSE seed_manual_stop END, updated_at = ? WHERE task_id = ? AND (seed_uploaded_bytes < ? OR seed_elapsed_seconds < ? OR seed_stop_reason IS NOT ?)")
        .bind(uploaded.max(0)).bind(elapsed.max(0)).bind(reason).bind(reason).bind(crate::models::task::now_iso()).bind(task_id).bind(uploaded.max(0)).bind(elapsed.max(0)).bind(reason).execute(&mut *tx).await.map_err(|e| e.to_string())?;
    if let Some(reason) = reason {
        sqlx::query("UPDATE torrent_runtime_snapshots SET seeding_state = ?, metadata_status = 'completed', upload_speed_bps = 0, seeding_enabled = (SELECT seeding_enabled FROM torrent_tasks WHERE task_id = ?), updated_at = ? WHERE task_id = ?")
            .bind(if reason == "limit" { "completed" } else if reason == "runtime_error" { "failed" } else { "stopped" }).bind(task_id).bind(crate::models::task::now_iso()).bind(task_id).execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn bt_seeding_restore_candidates(
    pool: &SqlitePool,
) -> Result<Vec<crate::models::TaskRecord>, String> {
    let ids = sqlx::query_scalar::<_, String>("SELECT t.id FROM tasks t JOIN torrent_tasks bt ON bt.task_id = t.id WHERE t.status = 'completed' AND bt.seeding_enabled = 1 AND bt.seed_manual_stop = 0 ORDER BY t.created_at").fetch_all(pool).await.map_err(|e| e.to_string())?;
    let mut tasks = Vec::new();
    for id in ids {
        if let Some(task) = crate::db::get_task_record(pool, &id).await? {
            tasks.push(task);
        }
    }
    Ok(tasks)
}

pub async fn persist_torrent_blob(
    pool: &SqlitePool,
    task_id: &str,
    bytes: &[u8],
) -> Result<(), String> {
    sqlx::query("UPDATE torrent_tasks SET torrent_blob = ?, updated_at = ? WHERE task_id = ?")
        .bind(bytes)
        .bind(crate::models::task::now_iso())
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
