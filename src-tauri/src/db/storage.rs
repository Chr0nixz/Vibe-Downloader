//! Storage sweep records: outcome log for artifact sweeps (startup and
//! manual cleanups) shown on the Storage & Cleanup Center page.

use sqlx::{Row, SqlitePool};

use crate::models::storage::StorageSweepRecord;

/// Keep only the most recent sweeps so the log cannot grow unboundedly.
const MAX_SWEEP_RECORDS: i64 = 20;

pub async fn insert_sweep_record(
    pool: &SqlitePool,
    record: &StorageSweepRecord,
    details_json: &str,
) -> Result<(), String> {
    let mut tx = crate::db::begin_immediate(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        INSERT INTO storage_sweeps (
            id, started_at, finished_at, mode, removed_count, failed_count,
            reclaimed_bytes, details_json
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.id)
    .bind(&record.started_at)
    .bind(&record.finished_at)
    .bind(&record.mode)
    .bind(i64::from(record.removed_count))
    .bind(i64::from(record.failed_count))
    .bind(record.reclaimed_bytes.parse::<i64>().unwrap_or(0))
    .bind(details_json)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // Bound the log: delete everything beyond the newest MAX_SWEEP_RECORDS.
    sqlx::query(
        r#"
        DELETE FROM storage_sweeps WHERE id IN (
            SELECT id FROM storage_sweeps
            ORDER BY finished_at DESC, started_at DESC
            LIMIT -1 OFFSET ?
        )
        "#,
    )
    .bind(MAX_SWEEP_RECORDS)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn latest_sweep_record(pool: &SqlitePool) -> Result<Option<StorageSweepRecord>, String> {
    let row = sqlx::query(
        r#"
        SELECT id, started_at, finished_at, mode, removed_count, failed_count,
               reclaimed_bytes
        FROM storage_sweeps
        ORDER BY finished_at DESC, started_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(sweep_record_from_row))
}

/// `(save_dir, average completed total_size)` pairs used by the Storage
/// Center to estimate how many average-sized tasks would still fit on each
/// volume. Sizes of zero are excluded so they cannot drag the average down.
pub async fn completed_avg_task_size_by_save_dir(
    pool: &SqlitePool,
) -> Result<Vec<(String, i64)>, String> {
    let rows = sqlx::query(
        r#"
        SELECT save_dir, AVG(total_size) AS avg_size
        FROM tasks
        WHERE status = 'completed' AND total_size > 0
        GROUP BY save_dir
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get("save_dir"), row.get("avg_size")))
        .collect())
}

fn sweep_record_from_row(row: sqlx::sqlite::SqliteRow) -> StorageSweepRecord {
    StorageSweepRecord {
        id: row.get("id"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        mode: row.get("mode"),
        removed_count: row.get::<i64, _>("removed_count").clamp(0, u32::MAX as i64) as u32,
        failed_count: row.get::<i64, _>("failed_count").clamp(0, u32::MAX as i64) as u32,
        reclaimed_bytes: row.get::<i64, _>("reclaimed_bytes").max(0).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_and_read_latest_sweep_record() {
        let db_path = std::env::temp_dir().join(format!(
            "vibe-storage-db-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let pool = crate::db::connect(&db_path).await.expect("test pool").pool;

        let record = StorageSweepRecord {
            id: "sweep-1".to_string(),
            started_at: "2026-09-13T10:00:00Z".to_string(),
            finished_at: "2026-09-13T10:00:02Z".to_string(),
            mode: "startup".to_string(),
            removed_count: 3,
            failed_count: 1,
            reclaimed_bytes: "4096".to_string(),
        };
        insert_sweep_record(&pool, &record, "{}")
            .await
            .expect("insert");

        let latest = latest_sweep_record(&pool)
            .await
            .expect("latest")
            .expect("row");
        assert_eq!(latest.id, "sweep-1");
        assert_eq!(latest.mode, "startup");
        assert_eq!(latest.reclaimed_bytes, "4096");

        // Newer record wins.
        let newer = StorageSweepRecord {
            id: "sweep-2".to_string(),
            started_at: "2026-09-13T11:00:00Z".to_string(),
            finished_at: "2026-09-13T11:00:01Z".to_string(),
            mode: "manual_orphans".to_string(),
            removed_count: 0,
            failed_count: 0,
            reclaimed_bytes: "0".to_string(),
        };
        insert_sweep_record(&pool, &newer, "{}")
            .await
            .expect("insert newer");
        let latest = latest_sweep_record(&pool)
            .await
            .expect("latest")
            .expect("row");
        assert_eq!(latest.id, "sweep-2");
    }

    #[tokio::test]
    async fn sweep_log_is_bounded() {
        let db_path = std::env::temp_dir().join(format!(
            "vibe-storage-bound-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let pool = crate::db::connect(&db_path).await.expect("test pool").pool;

        for index in 0..30 {
            let record = StorageSweepRecord {
                id: format!("sweep-{index}"),
                started_at: format!("2026-09-13T10:00:{index:02}Z"),
                finished_at: format!("2026-09-13T10:01:{index:02}Z"),
                mode: "startup".to_string(),
                removed_count: index,
                failed_count: 0,
                reclaimed_bytes: "0".to_string(),
            };
            insert_sweep_record(&pool, &record, "{}")
                .await
                .expect("insert");
        }
        let latest = latest_sweep_record(&pool)
            .await
            .expect("latest")
            .expect("row");
        // The newest record is the last inserted (monotonic timestamps).
        assert_eq!(latest.id, "sweep-29");
        assert_eq!(latest.removed_count, 29);
    }
}
