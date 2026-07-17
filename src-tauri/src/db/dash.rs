use std::collections::HashSet;

use sqlx::{Row, SqlitePool};

use crate::models::SegmentStatus;

#[derive(Debug, Clone)]
pub struct DashTaskRecord {
    pub task_id: String,
    pub input_url: String,
    pub manifest_url: String,
    pub staging_dir: String,
    pub video_representation_id: Option<String>,
    pub video_bandwidth: Option<i64>,
    pub video_codecs: Option<String>,
    pub audio_representation_id: Option<String>,
    pub audio_bandwidth: Option<i64>,
    pub audio_codecs: Option<String>,
    pub output_format: String,
    pub segment_count: i64,
    pub finish_requested: bool,
}

#[derive(Debug, Clone)]
pub struct DashTaskUpsert<'a> {
    pub task_id: &'a str,
    pub input_url: &'a str,
    pub manifest_url: &'a str,
    pub staging_dir: &'a str,
    pub video_representation_id: Option<&'a str>,
    pub video_bandwidth: Option<i64>,
    pub video_codecs: Option<&'a str>,
    pub audio_representation_id: Option<&'a str>,
    pub audio_bandwidth: Option<i64>,
    pub audio_codecs: Option<&'a str>,
    pub output_format: &'a str,
    pub segment_count: i64,
}

#[derive(Debug, Clone)]
pub struct DashSegmentRecord {
    pub id: String,
    pub task_id: String,
    pub track_kind: String,
    pub segment_index: i64,
    pub uri: String,
    pub local_path: String,
    pub byte_range_start: Option<i64>,
    pub byte_range_length: Option<i64>,
    pub init_segment_uri: Option<String>,
    pub init_segment_local_path: Option<String>,
    pub duration_ms: i64,
    pub downloaded_bytes: i64,
    pub status: SegmentStatus,
    pub retry_count: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DashSegmentUpsert<'a> {
    pub id: &'a str,
    pub task_id: &'a str,
    pub track_kind: &'a str,
    pub segment_index: i64,
    pub uri: &'a str,
    pub local_path: &'a str,
    pub byte_range_start: Option<i64>,
    pub byte_range_length: Option<i64>,
    pub init_segment_uri: Option<&'a str>,
    pub init_segment_local_path: Option<&'a str>,
    pub duration_ms: i64,
}

pub async fn upsert_dash_task(pool: &SqlitePool, task: DashTaskUpsert<'_>) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO dash_tasks (
            task_id, input_url, manifest_url, staging_dir,
            video_representation_id, video_bandwidth, video_codecs,
            audio_representation_id, audio_bandwidth, audio_codecs,
            output_format, segment_count, finish_requested, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            input_url = excluded.input_url,
            manifest_url = excluded.manifest_url,
            staging_dir = excluded.staging_dir,
            video_representation_id = excluded.video_representation_id,
            video_bandwidth = excluded.video_bandwidth,
            video_codecs = excluded.video_codecs,
            audio_representation_id = excluded.audio_representation_id,
            audio_bandwidth = excluded.audio_bandwidth,
            audio_codecs = excluded.audio_codecs,
            output_format = excluded.output_format,
            segment_count = excluded.segment_count,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(task.task_id)
    .bind(task.input_url)
    .bind(task.manifest_url)
    .bind(task.staging_dir)
    .bind(task.video_representation_id)
    .bind(task.video_bandwidth)
    .bind(task.video_codecs)
    .bind(task.audio_representation_id)
    .bind(task.audio_bandwidth)
    .bind(task.audio_codecs)
    .bind(task.output_format)
    .bind(task.segment_count)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_dash_task(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<DashTaskRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT task_id, input_url, manifest_url, staging_dir,
               video_representation_id, video_bandwidth, video_codecs,
               audio_representation_id, audio_bandwidth, audio_codecs,
               output_format, segment_count, finish_requested
        FROM dash_tasks
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|row| DashTaskRecord {
        task_id: row.get("task_id"),
        input_url: row.get("input_url"),
        manifest_url: row.get("manifest_url"),
        staging_dir: row.get("staging_dir"),
        video_representation_id: row.get("video_representation_id"),
        video_bandwidth: row.get("video_bandwidth"),
        video_codecs: row.get("video_codecs"),
        audio_representation_id: row.get("audio_representation_id"),
        audio_bandwidth: row.get("audio_bandwidth"),
        audio_codecs: row.get("audio_codecs"),
        output_format: row.get("output_format"),
        segment_count: row.get("segment_count"),
        finish_requested: row.get::<i64, _>("finish_requested") != 0,
    }))
}

pub async fn request_dash_finish(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        UPDATE dash_tasks
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

pub async fn dash_finish_requested(pool: &SqlitePool, task_id: &str) -> Result<bool, String> {
    let value = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT finish_requested FROM dash_tasks WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .flatten()
    .unwrap_or(0);
    Ok(value != 0)
}

pub async fn upsert_dash_segment(
    pool: &SqlitePool,
    segment: DashSegmentUpsert<'_>,
) -> Result<(), String> {
    let now = crate::models::task::now_iso();
    sqlx::query(
        r#"
        INSERT INTO dash_segments (
            id, task_id, track_kind, segment_index, uri, local_path,
            byte_range_start, byte_range_length,
            init_segment_uri, init_segment_local_path,
            duration_ms, downloaded_bytes, status, retry_count, last_error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'pending', 0, NULL, ?, ?)
        ON CONFLICT(task_id, track_kind, segment_index) DO UPDATE SET
            id = excluded.id,
            uri = excluded.uri,
            local_path = excluded.local_path,
            byte_range_start = excluded.byte_range_start,
            byte_range_length = excluded.byte_range_length,
            init_segment_uri = excluded.init_segment_uri,
            init_segment_local_path = excluded.init_segment_local_path,
            duration_ms = excluded.duration_ms,
            downloaded_bytes = CASE
                WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                THEN dash_segments.downloaded_bytes ELSE 0 END,
            status = CASE
                WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                THEN dash_segments.status ELSE 'pending' END,
            retry_count = CASE
                WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                THEN dash_segments.retry_count ELSE 0 END,
            last_error = CASE
                WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                THEN dash_segments.last_error ELSE NULL END,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(segment.id)
    .bind(segment.task_id)
    .bind(segment.track_kind)
    .bind(segment.segment_index)
    .bind(segment.uri)
    .bind(segment.local_path)
    .bind(segment.byte_range_start)
    .bind(segment.byte_range_length)
    .bind(segment.init_segment_uri)
    .bind(segment.init_segment_local_path)
    .bind(segment.duration_ms.max(0))
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// E-3: Bulk-insert DASH segment rows in a single transaction. Wraps the
/// per-row INSERT...ON CONFLICT in `pool.begin()/commit()` so N segments
/// produce one WAL commit instead of N. Callers passing 0 segments get a
/// no-op (no transaction is started).
pub async fn bulk_upsert_dash_segments(
    pool: &SqlitePool,
    segments: &[DashSegmentUpsert<'_>],
) -> Result<(), String> {
    if segments.is_empty() {
        return Ok(());
    }
    let now = crate::models::task::now_iso();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Could not begin DASH segment transaction: {e}"))?;
    for segment in segments {
        sqlx::query(
            r#"
            INSERT INTO dash_segments (
                id, task_id, track_kind, segment_index, uri, local_path,
                byte_range_start, byte_range_length,
                init_segment_uri, init_segment_local_path,
                duration_ms, downloaded_bytes, status, retry_count, last_error, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'pending', 0, NULL, ?, ?)
            ON CONFLICT(task_id, track_kind, segment_index) DO UPDATE SET
                id = excluded.id,
                uri = excluded.uri,
                local_path = excluded.local_path,
                byte_range_start = excluded.byte_range_start,
                byte_range_length = excluded.byte_range_length,
                init_segment_uri = excluded.init_segment_uri,
                init_segment_local_path = excluded.init_segment_local_path,
                duration_ms = excluded.duration_ms,
                downloaded_bytes = CASE
                    WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                    THEN dash_segments.downloaded_bytes ELSE 0 END,
                status = CASE
                    WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                    THEN dash_segments.status ELSE 'pending' END,
                retry_count = CASE
                    WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                    THEN dash_segments.retry_count ELSE 0 END,
                last_error = CASE
                    WHEN dash_segments.uri = excluded.uri AND dash_segments.local_path = excluded.local_path
                    THEN dash_segments.last_error ELSE NULL END,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(segment.id)
        .bind(segment.task_id)
        .bind(segment.track_kind)
        .bind(segment.segment_index)
        .bind(segment.uri)
        .bind(segment.local_path)
        .bind(segment.byte_range_start)
        .bind(segment.byte_range_length)
        .bind(segment.init_segment_uri)
        .bind(segment.init_segment_local_path)
        .bind(segment.duration_ms.max(0))
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("Could not commit DASH segment transaction: {e}"))?;
    Ok(())
}

pub async fn list_dash_segments(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<DashSegmentRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, track_kind, segment_index, uri, local_path,
               byte_range_start, byte_range_length,
               init_segment_uri, init_segment_local_path,
               duration_ms, downloaded_bytes, status, retry_count, last_error
        FROM dash_segments
        WHERE task_id = ?
        ORDER BY track_kind ASC, segment_index ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| DashSegmentRecord {
            id: row.get("id"),
            task_id: row.get("task_id"),
            track_kind: row.get("track_kind"),
            segment_index: row.get("segment_index"),
            uri: row.get("uri"),
            local_path: row.get("local_path"),
            byte_range_start: row.get("byte_range_start"),
            byte_range_length: row.get("byte_range_length"),
            init_segment_uri: row.get("init_segment_uri"),
            init_segment_local_path: row.get("init_segment_local_path"),
            duration_ms: row.get("duration_ms"),
            downloaded_bytes: row.get("downloaded_bytes"),
            status: SegmentStatus::from_db_str(row.get::<String, _>("status").as_str()),
            retry_count: row.get("retry_count"),
            last_error: row.get("last_error"),
        })
        .collect())
}

pub async fn update_dash_segment_status(
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
        UPDATE dash_segments
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

pub async fn reset_dash_segments_for_task(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM dash_segments WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        UPDATE dash_tasks
        SET finish_requested = 0, updated_at = ?
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

/// Sum `downloaded_bytes` of all completed dash_segments for resume accounting.
pub async fn existing_dash_downloaded_bytes(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<i64, String> {
    let value = sqlx::query_scalar::<_, Option<i64>>(
        r#"
        SELECT COALESCE(SUM(downloaded_bytes), 0)
        FROM dash_segments
        WHERE task_id = ? AND status = 'completed'
        "#,
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(value.unwrap_or(0))
}

/// Build a `(track_kind, segment_index)` set of completed segments for skip logic.
pub async fn existing_dash_segment_keys(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<HashSet<(String, i64)>, String> {
    let rows = sqlx::query(
        r#"
        SELECT track_kind, segment_index
        FROM dash_segments
        WHERE task_id = ? AND status = 'completed'
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<String, _>("track_kind"), row.get("segment_index")))
        .collect())
}
