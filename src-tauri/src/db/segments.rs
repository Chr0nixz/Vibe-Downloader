use sqlx::{Row, SqlitePool};

use crate::models::{AppSettings, SegmentStatus, SegmentSummary, TaskRecord, TaskSegmentRecord};

use super::{
    parse_multi_connection_threshold_bytes, planned_segments_for_task_with_plan,
    DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES, DEFAULT_SEGMENT_COUNT, MAX_SEGMENT_COUNT,
    MIN_SEGMENT_COUNT,
};

pub async fn insert_segment_record(
    pool: &SqlitePool,
    segment: &TaskSegmentRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO task_work_units (
            id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
            speed_bps, status, retry_count, last_error
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&segment.id)
    .bind(&segment.task_id)
    .bind(&segment.file_id)
    .bind(&segment.unit_kind)
    .bind(segment.range_start)
    .bind(segment.range_end)
    .bind(segment.downloaded_until)
    .bind(segment.speed_bps)
    .bind(segment.status.as_str())
    .bind(segment.retry_count)
    .bind(&segment.last_error)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn ensure_single_segment_for_task(
    pool: &SqlitePool,
    task: &TaskRecord,
) -> Result<TaskSegmentRecord, String> {
    ensure_task_segments(pool, task)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "Task segment could not be created.".to_string())
}

pub async fn ensure_task_segments(
    pool: &SqlitePool,
    task: &TaskRecord,
) -> Result<Vec<TaskSegmentRecord>, String> {
    ensure_task_segments_with_plan(
        pool,
        task,
        DEFAULT_MULTI_CONNECTION_THRESHOLD_BYTES,
        DEFAULT_SEGMENT_COUNT,
    )
    .await
}

pub async fn ensure_task_segments_with_settings(
    pool: &SqlitePool,
    task: &TaskRecord,
    settings: &AppSettings,
) -> Result<Vec<TaskSegmentRecord>, String> {
    ensure_task_segments_with_plan(
        pool,
        task,
        parse_multi_connection_threshold_bytes(&settings.multi_connection_threshold_bytes),
        settings
            .segment_count
            .min(settings.max_connections_per_host)
            .clamp(MIN_SEGMENT_COUNT, MAX_SEGMENT_COUNT),
    )
    .await
}

async fn ensure_task_segments_with_plan(
    pool: &SqlitePool,
    task: &TaskRecord,
    multi_connection_threshold_bytes: i64,
    segment_count: i32,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let existing = list_segment_records(pool, &task.id).await?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let task_work_units =
        planned_segments_for_task_with_plan(task, multi_connection_threshold_bytes, segment_count);
    for segment in &task_work_units {
        insert_segment_record(pool, segment).await?;
    }
    Ok(task_work_units)
}

pub async fn list_segment_records(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
        WHERE task_id = ?
        ORDER BY range_start ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_segment).collect()
}

pub async fn list_segment_records_paged(
    pool: &SqlitePool,
    task_id: &str,
    page: i64,
    page_size: i64,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let limit = page_size.clamp(1, 500);
    let offset = page.max(0).saturating_mul(limit);
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
        WHERE task_id = ?
        ORDER BY range_start ASC
        LIMIT ? OFFSET ?
        "#,
    )
    .bind(task_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_segment).collect()
}

pub async fn list_segment_records_cursor(
    pool: &SqlitePool,
    task_id: &str,
    after_range_start: Option<i64>,
    limit: i64,
) -> Result<Vec<TaskSegmentRecord>, String> {
    let limit = limit.clamp(1, 500);
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
        WHERE task_id = ? AND (? IS NULL OR range_start > ?)
        ORDER BY range_start ASC
        LIMIT ?
        "#,
    )
    .bind(task_id)
    .bind(after_range_start)
    .bind(after_range_start)
    .bind(limit + 1)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_segment).collect()
}

pub async fn segment_summary(pool: &SqlitePool, task_id: &str) -> Result<SegmentSummary, String> {
    let rows = list_segment_records(pool, task_id).await?;
    let total = i32::try_from(rows.len()).unwrap_or(i32::MAX);
    let active = i32::try_from(
        rows.iter()
            .filter(|segment| segment.status == SegmentStatus::Downloading)
            .count(),
    )
    .unwrap_or(i32::MAX);
    let completed = i32::try_from(
        rows.iter()
            .filter(|segment| segment.status == SegmentStatus::Completed)
            .count(),
    )
    .unwrap_or(i32::MAX);
    let failed = i32::try_from(
        rows.iter()
            .filter(|segment| segment.status == SegmentStatus::Failed)
            .count(),
    )
    .unwrap_or(i32::MAX);
    Ok(SegmentSummary {
        total,
        active,
        completed,
        failed,
        downloaded_bytes: total_segment_downloaded_bytes(&rows).to_string(),
        speed_bps: rows
            .iter()
            .map(|segment| segment.speed_bps)
            .sum::<i64>()
            .to_string(),
    })
}

pub async fn get_first_segment_record(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<TaskSegmentRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
        WHERE task_id = ?
        ORDER BY range_start ASC
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    row.as_ref().map(row_to_segment).transpose()
}

fn row_to_segment(row: &sqlx::sqlite::SqliteRow) -> Result<TaskSegmentRecord, String> {
    Ok(TaskSegmentRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        file_id: row.get("file_id"),
        unit_kind: row.get("unit_kind"),
        range_start: row.get("range_start"),
        range_end: row.get("range_end"),
        downloaded_until: row.get("downloaded_until"),
        speed_bps: row.get("speed_bps"),
        status: SegmentStatus::from_db_str(row.get::<String, _>("status").as_str()),
        retry_count: row.get("retry_count"),
        last_error: row.get("last_error"),
    })
}

pub async fn update_segment_progress(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
    status: SegmentStatus,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = 0, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(status.as_str())
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_runtime_progress(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
    speed_bps: i64,
    status: SegmentStatus,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = ?, status = ?, last_error = NULL
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(speed_bps.max(0))
    .bind(status.as_str())
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_downloaded_until(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_range_end(
    pool: &SqlitePool,
    segment_id: &str,
    range_end: i64,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET range_end = ?
        WHERE id = ?
        "#,
    )
    .bind(range_end)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segment_retry(
    pool: &SqlitePool,
    segment_id: &str,
    downloaded_until: i64,
    retry_count: i32,
    last_error: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET downloaded_until = ?, speed_bps = 0, retry_count = ?, status = 'pending', last_error = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_until)
    .bind(retry_count)
    .bind(last_error)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub struct SegmentSplit {
    pub original_segment_id: String,
    pub original_range_end: i64,
    pub tail_segment: TaskSegmentRecord,
}

// Split the segment with the most bytes left, halving its REMAINING (not total) range so
// the active worker keeps its already-downloaded prefix. The UPDATE + INSERT run in one
// transaction so a concurrent coordinator never observes a gap or overlap in range coverage.
pub async fn split_largest_remaining_segment(
    pool: &SqlitePool,
    task_id: &str,
    min_remaining_bytes: i64,
    max_segments: usize,
) -> Result<Option<SegmentSplit>, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM task_work_units WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= max_segments {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }

    let row = sqlx::query(
        r#"
        SELECT id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
               speed_bps, status, retry_count, last_error
        FROM task_work_units
        WHERE task_id = ? AND downloaded_until <= range_end
        ORDER BY (range_end - downloaded_until) DESC
        LIMIT 1
        "#,
    )
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    let Some(row) = row else {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    };
    let segment = row_to_segment(&row)?;
    let remaining = segment
        .range_end
        .saturating_sub(segment.downloaded_until)
        .saturating_add(1);
    if remaining < min_remaining_bytes {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }

    let first_remaining_len = remaining / 2;
    if first_remaining_len <= 0 {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let original_range_end = segment.downloaded_until + first_remaining_len - 1;
    let tail_start = original_range_end + 1;
    if tail_start > segment.range_end {
        tx.rollback().await.map_err(|e| e.to_string())?;
        return Ok(None);
    }

    sqlx::query("UPDATE task_work_units SET range_end = ? WHERE id = ?")
        .bind(original_range_end)
        .bind(&segment.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;

    let tail_segment = TaskSegmentRecord {
        id: uuid::Uuid::new_v4().to_string(),
        task_id: segment.task_id.clone(),
        file_id: segment.file_id.clone(),
        unit_kind: segment.unit_kind.clone(),
        range_start: tail_start,
        range_end: segment.range_end,
        downloaded_until: tail_start,
        speed_bps: 0,
        status: SegmentStatus::Pending,
        retry_count: 0,
        last_error: None,
    };

    sqlx::query(
        r#"
        INSERT INTO task_work_units (
            id, task_id, file_id, unit_kind, range_start, range_end, downloaded_until,
            speed_bps, status, retry_count, last_error
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&tail_segment.id)
    .bind(&tail_segment.task_id)
    .bind(&tail_segment.file_id)
    .bind(&tail_segment.unit_kind)
    .bind(tail_segment.range_start)
    .bind(tail_segment.range_end)
    .bind(tail_segment.downloaded_until)
    .bind(tail_segment.speed_bps)
    .bind(tail_segment.status.as_str())
    .bind(tail_segment.retry_count)
    .bind(&tail_segment.last_error)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(Some(SegmentSplit {
        original_segment_id: segment.id,
        original_range_end,
        tail_segment,
    }))
}

pub async fn update_segment_status(
    pool: &SqlitePool,
    segment_id: &str,
    status: SegmentStatus,
    downloaded_until: Option<i64>,
    last_error: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET status = ?, downloaded_until = COALESCE(?, downloaded_until),
            speed_bps = CASE WHEN ? = 'downloading' THEN speed_bps ELSE 0 END,
            last_error = ?
        WHERE id = ?
        "#,
    )
    .bind(status.as_str())
    .bind(downloaded_until)
    .bind(status.as_str())
    .bind(last_error)
    .bind(segment_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segments_status_for_task(
    pool: &SqlitePool,
    task_id: &str,
    status: SegmentStatus,
    last_error: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET status = ?, last_error = ?,
            speed_bps = CASE WHEN ? = 'downloading' THEN speed_bps ELSE 0 END
        WHERE task_id = ? AND status != 'completed'
        "#,
    )
    .bind(status.as_str())
    .bind(last_error)
    .bind(status.as_str())
    .bind(task_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_segments_status_for_task_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    status: SegmentStatus,
    last_error: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_work_units
        SET status = ?, last_error = ?,
            speed_bps = CASE WHEN ? = 'downloading' THEN speed_bps ELSE 0 END
        WHERE task_id = ? AND status != 'completed'
        "#,
    )
    .bind(status.as_str())
    .bind(last_error)
    .bind(status.as_str())
    .bind(task_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn segment_downloaded_bytes(segment: &TaskSegmentRecord) -> i64 {
    let next_byte = segment
        .downloaded_until
        .clamp(segment.range_start, segment.range_end.saturating_add(1));
    next_byte.saturating_sub(segment.range_start)
}

pub fn total_segment_downloaded_bytes(task_work_units: &[TaskSegmentRecord]) -> i64 {
    task_work_units.iter().map(segment_downloaded_bytes).sum()
}
