use sqlx::{Row, SqlitePool};

use crate::models::SegmentStatus;

#[derive(Debug, Clone)]
pub struct HlsTaskRecord {
    pub task_id: String,
    pub input_url: String,
    pub media_url: String,
    pub playlist_kind: String,
    pub selected_bandwidth: Option<i64>,
    pub selected_resolution: Option<String>,
    pub target_duration: i64,
    pub last_media_sequence: Option<i64>,
    pub output_format: String,
    pub staging_dir: String,
    pub finish_requested: bool,
}

#[derive(Debug, Clone)]
pub struct HlsTaskUpsert<'a> {
    pub task_id: &'a str,
    pub input_url: &'a str,
    pub media_url: &'a str,
    pub playlist_kind: &'a str,
    pub selected_bandwidth: Option<i64>,
    pub selected_resolution: Option<&'a str>,
    pub target_duration: i64,
    pub last_media_sequence: Option<i64>,
    pub output_format: &'a str,
    pub staging_dir: &'a str,
}

#[derive(Debug, Clone)]
pub struct HlsSegmentRecord {
    pub id: String,
    pub task_id: String,
    pub media_sequence: i64,
    pub discontinuity_sequence: i64,
    pub uri: String,
    pub local_path: String,
    pub duration_ms: i64,
    pub byte_range_start: Option<i64>,
    pub byte_range_length: Option<i64>,
    pub key_method: Option<String>,
    pub key_uri: Option<String>,
    pub key_iv: Option<String>,
    pub downloaded_bytes: i64,
    pub status: SegmentStatus,
    pub retry_count: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HlsSegmentUpsert<'a> {
    pub id: &'a str,
    pub task_id: &'a str,
    pub media_sequence: i64,
    pub discontinuity_sequence: i64,
    pub uri: &'a str,
    pub local_path: &'a str,
    pub duration_ms: i64,
    pub byte_range_start: Option<i64>,
    pub byte_range_length: Option<i64>,
    pub key_method: Option<&'a str>,
    pub key_uri: Option<&'a str>,
    pub key_iv: Option<&'a str>,
}

pub async fn upsert_hls_task(pool: &SqlitePool, task: HlsTaskUpsert<'_>) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO hls_tasks (
            task_id, input_url, media_url, playlist_kind, selected_bandwidth,
            selected_resolution, target_duration, last_media_sequence, output_format,
            staging_dir, finish_requested, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            media_url = excluded.media_url,
            playlist_kind = excluded.playlist_kind,
            selected_bandwidth = excluded.selected_bandwidth,
            selected_resolution = excluded.selected_resolution,
            target_duration = excluded.target_duration,
            last_media_sequence = COALESCE(excluded.last_media_sequence, hls_tasks.last_media_sequence),
            output_format = excluded.output_format,
            staging_dir = excluded.staging_dir,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task.task_id)
    .bind(task.input_url)
    .bind(task.media_url)
    .bind(task.playlist_kind)
    .bind(task.selected_bandwidth)
    .bind(task.selected_resolution)
    .bind(task.target_duration.max(1))
    .bind(task.last_media_sequence)
    .bind(task.output_format)
    .bind(task.staging_dir)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_hls_task(pool: &SqlitePool, task_id: &str) -> Result<Option<HlsTaskRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT task_id, input_url, media_url, playlist_kind, selected_bandwidth,
               selected_resolution, target_duration, last_media_sequence, output_format,
               staging_dir, finish_requested
        FROM hls_tasks
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|row| HlsTaskRecord {
        task_id: row.get("task_id"),
        input_url: row.get("input_url"),
        media_url: row.get("media_url"),
        playlist_kind: row.get("playlist_kind"),
        selected_bandwidth: row.get("selected_bandwidth"),
        selected_resolution: row.get("selected_resolution"),
        target_duration: row.get("target_duration"),
        last_media_sequence: row.get("last_media_sequence"),
        output_format: row.get("output_format"),
        staging_dir: row.get("staging_dir"),
        finish_requested: row.get::<i64, _>("finish_requested") != 0,
    }))
}

pub async fn request_hls_finish(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        UPDATE hls_tasks
        SET finish_requested = 1, updated_at = ?
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

pub async fn hls_finish_requested(pool: &SqlitePool, task_id: &str) -> Result<bool, String> {
    let value = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT finish_requested FROM hls_tasks WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .flatten()
    .unwrap_or(0);
    Ok(value != 0)
}

pub async fn update_hls_last_media_sequence(
    pool: &SqlitePool,
    task_id: &str,
    sequence: i64,
) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        UPDATE hls_tasks
        SET last_media_sequence = MAX(COALESCE(last_media_sequence, -1), ?), updated_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(sequence)
    .bind(&now)
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn upsert_hls_segment(
    pool: &SqlitePool,
    segment: HlsSegmentUpsert<'_>,
) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO hls_segments (
            id, task_id, media_sequence, discontinuity_sequence, uri, local_path,
            duration_ms, byte_range_start, byte_range_length, key_method, key_uri, key_iv,
            downloaded_bytes, status, retry_count, last_error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'pending', 0, NULL, ?, ?)
        ON CONFLICT(task_id, media_sequence, discontinuity_sequence) DO UPDATE SET
            id = excluded.id,
            uri = excluded.uri,
            local_path = excluded.local_path,
            duration_ms = excluded.duration_ms,
            byte_range_start = excluded.byte_range_start,
            byte_range_length = excluded.byte_range_length,
            key_method = excluded.key_method,
            key_uri = excluded.key_uri,
            key_iv = excluded.key_iv,
            downloaded_bytes = 0,
            status = 'pending',
            retry_count = 0,
            last_error = NULL,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(segment.id)
    .bind(segment.task_id)
    .bind(segment.media_sequence)
    .bind(segment.discontinuity_sequence)
    .bind(segment.uri)
    .bind(segment.local_path)
    .bind(segment.duration_ms.max(0))
    .bind(segment.byte_range_start)
    .bind(segment.byte_range_length)
    .bind(segment.key_method)
    .bind(segment.key_uri)
    .bind(segment.key_iv)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn list_hls_segments(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<HlsSegmentRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, media_sequence, discontinuity_sequence, uri, local_path,
               duration_ms, byte_range_start, byte_range_length, key_method, key_uri,
               key_iv, downloaded_bytes, status, retry_count, last_error
        FROM hls_segments
        WHERE task_id = ?
        ORDER BY discontinuity_sequence ASC, media_sequence ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| HlsSegmentRecord {
            id: row.get("id"),
            task_id: row.get("task_id"),
            media_sequence: row.get("media_sequence"),
            discontinuity_sequence: row.get("discontinuity_sequence"),
            uri: row.get("uri"),
            local_path: row.get("local_path"),
            duration_ms: row.get("duration_ms"),
            byte_range_start: row.get("byte_range_start"),
            byte_range_length: row.get("byte_range_length"),
            key_method: row.get("key_method"),
            key_uri: row.get("key_uri"),
            key_iv: row.get("key_iv"),
            downloaded_bytes: row.get("downloaded_bytes"),
            status: SegmentStatus::from_db_str(row.get::<String, _>("status").as_str()),
            retry_count: row.get("retry_count"),
            last_error: row.get("last_error"),
        })
        .collect())
}

pub async fn update_hls_segment_status(
    pool: &SqlitePool,
    id: &str,
    downloaded_bytes: i64,
    status: SegmentStatus,
    retry_count: i32,
    last_error: Option<&str>,
) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        UPDATE hls_segments
        SET downloaded_bytes = ?, status = ?, retry_count = ?, last_error = ?, updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes.max(0))
    .bind(status.as_str())
    .bind(retry_count.max(0))
    .bind(last_error)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn reset_hls_segments_for_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM hls_segments WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        UPDATE hls_tasks
        SET last_media_sequence = NULL, finish_requested = 0, updated_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(crate::models::task::now_iso())
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}
