use sqlx::{Row, SqlitePool};

use crate::models::task::now_iso;

/// Cooldown applied when a mirror fails. Within this window the mirror is
/// excluded from `list_healthy_mirrors_for_file` but remains available to
/// the serial full-file fallback path in the Metalink engine.
pub const METALINK_MIRROR_COOLDOWN_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct MetalinkTaskUpsert<'a> {
    pub task_id: &'a str,
    pub manifest_url: &'a str,
    pub manifest_format: &'a str,
    pub file_count: i64,
}

#[derive(Debug, Clone)]
pub struct MetalinkResourceRecord {
    pub id: String,
    pub task_id: String,
    pub file_id: String,
    pub url: String,
    pub priority: i64,
    pub location: Option<String>,
    pub status: String,
    pub failure_count: i64,
    pub last_error: Option<String>,
    /// ISO timestamp of the most recent attempt (success or failure).
    /// `None` until the mirror has been attempted at least once.
    pub last_attempt_at: Option<String>,
    /// ISO timestamp until which the mirror should be skipped. `None`
    /// when the mirror is healthy (no cooldown active).
    pub cooldown_until: Option<String>,
    /// Rolling average throughput observed for this mirror, in bytes/sec.
    /// 0 until the mirror has been attempted.
    pub avg_speed_bps: i64,
    /// 1 when the mirror advertises HTTP Range support, 0 after a 416
    /// response proves it does not. Once 0, the parallel path will
    /// route this mirror only to full-file fallback work units.
    pub supports_range: bool,
}

#[derive(Debug, Clone)]
pub struct MetalinkResourceInsert<'a> {
    pub id: &'a str,
    pub task_id: &'a str,
    pub file_id: &'a str,
    pub url: &'a str,
    pub priority: i64,
    pub location: Option<&'a str>,
}

pub async fn upsert_metalink_task(
    pool: &SqlitePool,
    input: MetalinkTaskUpsert<'_>,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        INSERT INTO metalink_tasks (
            task_id, manifest_url, manifest_format, file_count, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            manifest_url = excluded.manifest_url,
            manifest_format = excluded.manifest_format,
            file_count = excluded.file_count,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(input.task_id)
    .bind(input.manifest_url)
    .bind(input.manifest_format)
    .bind(input.file_count.max(0))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn insert_metalink_resource(
    pool: &SqlitePool,
    input: MetalinkResourceInsert<'_>,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO metalink_resources (
            id, task_id, file_id, url, priority, location, status,
            failure_count, last_error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 'pending', 0, NULL, ?, ?)
        "#,
    )
    .bind(input.id)
    .bind(input.task_id)
    .bind(input.file_id)
    .bind(input.url)
    .bind(input.priority)
    .bind(input.location)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list_metalink_resources_for_file(
    pool: &SqlitePool,
    file_id: &str,
) -> Result<Vec<MetalinkResourceRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, url, priority, location, status,
               failure_count, last_error, last_attempt_at, cooldown_until,
               avg_speed_bps, supports_range
        FROM metalink_resources
        WHERE file_id = ?
        ORDER BY priority ASC, rowid ASC
        "#,
    )
    .bind(file_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(row_to_resource).collect())
}

pub async fn mark_metalink_resource_failed(
    pool: &SqlitePool,
    id: &str,
    error: &str,
) -> Result<(), String> {
    let now = now_iso();
    // Cooldown is bounded: after METALINK_MIRROR_COOLDOWN_SECS the mirror
    // becomes eligible again. The caller (engine) decides the duration
    // via `set_metalink_mirror_cooldown`; for non-range failures we
    // apply the default cooldown by setting cooldown_until = now + N secs,
    // which `list_healthy_mirrors_for_file` filters on.
    let cooldown_until = now_iso_plus_seconds(METALINK_MIRROR_COOLDOWN_SECS);
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET status = 'failed',
            failure_count = failure_count + 1,
            last_error = ?,
            last_attempt_at = ?,
            cooldown_until = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(error)
    .bind(&now)
    .bind(&cooldown_until)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Mark the mirror as attempted (sets `last_attempt_at` to now) without
/// flagging a failure. The engine calls this just before sending a Range
/// request so that subsequent worker iterations see the mirror as
/// "in flight" and prefer alternatives.
pub async fn mark_metalink_resource_attempted(
    pool: &SqlitePool,
    id: &str,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET last_attempt_at = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Set the mirror's cooldown_until to a specific timestamp (ISO 8601).
/// Used by the parallel worker when a 416 or 403 response proves the
/// mirror is unavailable for range requests but should not be poisoned
/// forever — it can still serve a full-file GET later if needed.
pub async fn set_metalink_mirror_cooldown(
    pool: &SqlitePool,
    id: &str,
    cooldown_until_iso: &str,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET cooldown_until = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(cooldown_until_iso)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Update the rolling average speed observed for a mirror after a
/// successful (or partial) transfer. The engine computes the new
/// average as `0.5 * old + 0.5 * observed` to smooth spikes.
pub async fn update_mirror_speed(
    pool: &SqlitePool,
    id: &str,
    avg_speed_bps: i64,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET avg_speed_bps = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(avg_speed_bps)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Mark a mirror as not supporting HTTP Range requests (set
/// `supports_range = 0`). Called when a 416 response is observed.
/// Subsequent calls to `list_healthy_mirrors_for_file` will exclude
/// this mirror from the parallel-eligible pool, but the serial
/// fallback path (`list_metalink_resources_for_file`) still returns it
/// so a full-file GET can succeed.
pub async fn mark_mirror_unsupported_range(
    pool: &SqlitePool,
    id: &str,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET supports_range = 0, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Return only mirrors that are eligible for parallel range requests:
/// not failed, not in cooldown, and known to support HTTP Range.
/// Failed mirrors that have served their cooldown are re-included
/// (their `failure_count` is preserved for prioritization).
pub async fn list_healthy_mirrors_for_file(
    pool: &SqlitePool,
    file_id: &str,
) -> Result<Vec<MetalinkResourceRecord>, String> {
    let now = now_iso();
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, url, priority, location, status,
               failure_count, last_error, last_attempt_at, cooldown_until,
               avg_speed_bps, supports_range
        FROM metalink_resources
        WHERE file_id = ?
          AND supports_range = 1
          AND (cooldown_until IS NULL OR cooldown_until <= ?)
          AND status != 'completed'
        ORDER BY priority ASC,
                 failure_count ASC,
                 avg_speed_bps DESC,
                 rowid ASC
        "#,
    )
    .bind(file_id)
    .bind(&now)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(row_to_resource).collect())
}

pub async fn mark_metalink_resource_completed(pool: &SqlitePool, id: &str) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET status = 'completed',
            last_error = NULL,
            last_attempt_at = ?,
            cooldown_until = NULL,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&now)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn row_to_resource(row: sqlx::sqlite::SqliteRow) -> MetalinkResourceRecord {
    MetalinkResourceRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        file_id: row.get("file_id"),
        url: row.get("url"),
        priority: row.get("priority"),
        location: row.get("location"),
        status: row.get("status"),
        failure_count: row.get("failure_count"),
        last_error: row.get("last_error"),
        last_attempt_at: row.get("last_attempt_at"),
        cooldown_until: row.get("cooldown_until"),
        avg_speed_bps: row.get("avg_speed_bps"),
        supports_range: row.get::<i64, _>("supports_range") != 0,
    }
}

pub async fn list_metalink_resources_for_task(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<MetalinkResourceRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, url, priority, location, status,
               failure_count, last_error, last_attempt_at, cooldown_until,
               avg_speed_bps, supports_range
        FROM metalink_resources
        WHERE task_id = ?
        ORDER BY priority ASC, rowid ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows.into_iter().map(row_to_resource).collect())
}

pub async fn reset_metalink_resource_statuses(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET status = 'pending',
            failure_count = 0,
            last_error = NULL,
            last_attempt_at = NULL,
            cooldown_until = NULL,
            updated_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(&now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Return an ISO 8601 timestamp `seconds` in the future from now.
/// Used to populate `cooldown_until` when a mirror is temporarily
/// excluded after a transient failure.
fn now_iso_plus_seconds(seconds: u64) -> String {
    use chrono::Utc;
    let now = Utc::now();
    let future = now + chrono::Duration::seconds(seconds.min(i64::MAX as u64) as i64);
    future.to_rfc3339()
}

/// Boosts the preferred mirror's priority to 0 (tried first) so the Metalink
/// engine selects it before other mirrors on the next download attempt.
pub async fn promote_metalink_resource_for_retry(
    pool: &SqlitePool,
    task_id: &str,
    mirror_url: &str,
) -> Result<(), String> {
    let now = now_iso();
    sqlx::query(
        r#"
        UPDATE metalink_resources
        SET priority = 0, updated_at = ?
        WHERE task_id = ? AND url = ?
        "#,
    )
    .bind(&now)
    .bind(task_id)
    .bind(mirror_url)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
