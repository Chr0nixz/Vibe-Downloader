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
    /// F-6: JSON array of selected audio track URIs, or NULL.
    pub selected_audio_track_uris: Option<String>,
    /// F-6: JSON array of selected subtitle track URIs, or NULL.
    pub selected_subtitle_track_uris: Option<String>,
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
    /// F-6: JSON array of selected audio track URIs, or None.
    pub selected_audio_track_uris: Option<&'a str>,
    /// F-6: JSON array of selected subtitle track URIs, or None.
    pub selected_subtitle_track_uris: Option<&'a str>,
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
    pub init_map_uri: Option<String>,
    pub init_map_local_path: Option<String>,
    pub init_map_byte_range_start: Option<i64>,
    pub init_map_byte_range_length: Option<i64>,
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
    pub init_map_uri: Option<&'a str>,
    pub init_map_local_path: Option<&'a str>,
    pub init_map_byte_range_start: Option<i64>,
    pub init_map_byte_range_length: Option<i64>,
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
            staging_dir, selected_audio_track_uris, selected_subtitle_track_uris,
            finish_requested, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            media_url = excluded.media_url,
            playlist_kind = excluded.playlist_kind,
            selected_bandwidth = excluded.selected_bandwidth,
            selected_resolution = excluded.selected_resolution,
            target_duration = excluded.target_duration,
            last_media_sequence = COALESCE(excluded.last_media_sequence, hls_tasks.last_media_sequence),
            output_format = excluded.output_format,
            staging_dir = excluded.staging_dir,
            selected_audio_track_uris = COALESCE(excluded.selected_audio_track_uris, hls_tasks.selected_audio_track_uris),
            selected_subtitle_track_uris = COALESCE(excluded.selected_subtitle_track_uris, hls_tasks.selected_subtitle_track_uris),
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
    .bind(task.selected_audio_track_uris)
    .bind(task.selected_subtitle_track_uris)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn get_hls_task(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<HlsTaskRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT task_id, input_url, media_url, playlist_kind, selected_bandwidth,
               selected_resolution, target_duration, last_media_sequence, output_format,
               staging_dir, finish_requested,
               selected_audio_track_uris, selected_subtitle_track_uris
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
        selected_audio_track_uris: row.get("selected_audio_track_uris"),
        selected_subtitle_track_uris: row.get("selected_subtitle_track_uris"),
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
            duration_ms, byte_range_start, byte_range_length,
            init_map_uri, init_map_local_path, init_map_byte_range_start, init_map_byte_range_length,
            key_method, key_uri, key_iv,
            downloaded_bytes, status, retry_count, last_error, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'pending', 0, NULL, ?, ?)
        ON CONFLICT(task_id, media_sequence, discontinuity_sequence) DO UPDATE SET
            id = excluded.id,
            uri = excluded.uri,
            local_path = excluded.local_path,
            duration_ms = excluded.duration_ms,
            byte_range_start = excluded.byte_range_start,
            byte_range_length = excluded.byte_range_length,
            init_map_uri = excluded.init_map_uri,
            init_map_local_path = excluded.init_map_local_path,
            init_map_byte_range_start = excluded.init_map_byte_range_start,
            init_map_byte_range_length = excluded.init_map_byte_range_length,
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
    .bind(segment.init_map_uri)
    .bind(segment.init_map_local_path)
    .bind(segment.init_map_byte_range_start)
    .bind(segment.init_map_byte_range_length)
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

/// E-3: Bulk-insert HLS segment rows in a single transaction. Wraps the
/// per-row INSERT...ON CONFLICT in `pool.begin()/commit()` so N segments
/// produce one WAL commit instead of N. Callers passing 0 segments get a
/// no-op (no transaction is started).
pub async fn bulk_upsert_hls_segments(
    pool: &SqlitePool,
    segments: &[HlsSegmentUpsert<'_>],
) -> Result<(), String> {
    if segments.is_empty() {
        return Ok(());
    }
    let now = crate::models::task::now_iso();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Could not begin HLS segment transaction: {e}"))?;
    for segment in segments {
        sqlx::query(
            r#"
            INSERT INTO hls_segments (
                id, task_id, media_sequence, discontinuity_sequence, uri, local_path,
                duration_ms, byte_range_start, byte_range_length,
                init_map_uri, init_map_local_path, init_map_byte_range_start, init_map_byte_range_length,
                key_method, key_uri, key_iv,
                downloaded_bytes, status, retry_count, last_error, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 'pending', 0, NULL, ?, ?)
            ON CONFLICT(task_id, media_sequence, discontinuity_sequence) DO UPDATE SET
                id = excluded.id,
                uri = excluded.uri,
                local_path = excluded.local_path,
                duration_ms = excluded.duration_ms,
                byte_range_start = excluded.byte_range_start,
                byte_range_length = excluded.byte_range_length,
                init_map_uri = excluded.init_map_uri,
                init_map_local_path = excluded.init_map_local_path,
                init_map_byte_range_start = excluded.init_map_byte_range_start,
                init_map_byte_range_length = excluded.init_map_byte_range_length,
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
        .bind(segment.init_map_uri)
        .bind(segment.init_map_local_path)
        .bind(segment.init_map_byte_range_start)
        .bind(segment.init_map_byte_range_length)
        .bind(segment.key_method)
        .bind(segment.key_uri)
        .bind(segment.key_iv)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit()
        .await
        .map_err(|e| format!("Could not commit HLS segment transaction: {e}"))?;
    Ok(())
}

pub async fn list_hls_segments(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<HlsSegmentRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, media_sequence, discontinuity_sequence, uri, local_path,
               duration_ms, byte_range_start, byte_range_length,
               init_map_uri, init_map_local_path, init_map_byte_range_start, init_map_byte_range_length,
               key_method, key_uri, key_iv, downloaded_bytes, status, retry_count, last_error
        FROM hls_segments
        WHERE task_id = ?
        ORDER BY discontinuity_sequence ASC, media_sequence ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(row_to_hls_segment).collect())
}

/// Cursor is `discontinuity_sequence:media_sequence` of the last returned row.
pub async fn list_hls_segments_page(
    pool: &SqlitePool,
    task_id: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<Vec<HlsSegmentRecord>, String> {
    let limit = limit.max(1);
    let (after_disc, after_seq) = parse_hls_segment_cursor(cursor);
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, media_sequence, discontinuity_sequence, uri, local_path,
               duration_ms, byte_range_start, byte_range_length,
               init_map_uri, init_map_local_path, init_map_byte_range_start, init_map_byte_range_length,
               key_method, key_uri, key_iv, downloaded_bytes, status, retry_count, last_error
        FROM hls_segments
        WHERE task_id = ?
          AND (
            ? IS NULL
            OR discontinuity_sequence > ?
            OR (discontinuity_sequence = ? AND media_sequence > ?)
          )
        ORDER BY discontinuity_sequence ASC, media_sequence ASC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(after_disc)
    .bind(after_disc)
    .bind(after_disc)
    .bind(after_seq)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(row_to_hls_segment).collect())
}

fn row_to_hls_segment(row: sqlx::sqlite::SqliteRow) -> HlsSegmentRecord {
    HlsSegmentRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        media_sequence: row.get("media_sequence"),
        discontinuity_sequence: row.get("discontinuity_sequence"),
        uri: row.get("uri"),
        local_path: row.get("local_path"),
        duration_ms: row.get("duration_ms"),
        byte_range_start: row.get("byte_range_start"),
        byte_range_length: row.get("byte_range_length"),
        init_map_uri: row.get("init_map_uri"),
        init_map_local_path: row.get("init_map_local_path"),
        init_map_byte_range_start: row.get("init_map_byte_range_start"),
        init_map_byte_range_length: row.get("init_map_byte_range_length"),
        key_method: row.get("key_method"),
        key_uri: row.get("key_uri"),
        key_iv: row.get("key_iv"),
        downloaded_bytes: row.get("downloaded_bytes"),
        status: SegmentStatus::from_db_str(row.get::<String, _>("status").as_str()),
        retry_count: row.get("retry_count"),
        last_error: row.get("last_error"),
    }
}

fn parse_hls_segment_cursor(cursor: Option<&str>) -> (Option<i64>, i64) {
    let Some(raw) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return (None, -1);
    };
    if let Some((disc, seq)) = raw.split_once(':') {
        let disc = disc.parse::<i64>().ok();
        let seq = seq.parse::<i64>().unwrap_or(-1);
        return (disc, seq);
    }
    (Some(0), raw.parse::<i64>().unwrap_or(-1))
}

pub fn hls_segment_cursor(record: &HlsSegmentRecord) -> String {
    format!(
        "{}:{}",
        record.discontinuity_sequence, record.media_sequence
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SegmentStatus;

    #[test]
    fn parse_hls_segment_cursor_accepts_composite_and_legacy() {
        assert_eq!(parse_hls_segment_cursor(None), (None, -1));
        assert_eq!(parse_hls_segment_cursor(Some("2:10")), (Some(2), 10));
        assert_eq!(parse_hls_segment_cursor(Some("7")), (Some(0), 7));
    }

    #[test]
    fn hls_segment_cursor_formats_pair() {
        let record = HlsSegmentRecord {
            id: "s1".into(),
            task_id: "t1".into(),
            media_sequence: 42,
            discontinuity_sequence: 1,
            uri: "https://cdn.example/a.ts".into(),
            local_path: "a.ts".into(),
            duration_ms: 1000,
            byte_range_start: None,
            byte_range_length: None,
            init_map_uri: None,
            init_map_local_path: None,
            init_map_byte_range_start: None,
            init_map_byte_range_length: None,
            key_method: None,
            key_uri: None,
            key_iv: None,
            downloaded_bytes: 0,
            status: SegmentStatus::Pending,
            retry_count: 0,
            last_error: None,
        };
        assert_eq!(hls_segment_cursor(&record), "1:42");
    }
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
