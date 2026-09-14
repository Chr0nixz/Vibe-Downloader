//! Recovery history: durable log of recovery resolutions (per-task resolve,
//! batch safe retry, credential updates) shown on the Recovery Center page.
//! Unlike task_events (pruned to 14 days / 200 rows per task), this log is
//! the long-lived audit trail; it is bounded by prune-on-insert only.

use sqlx::{Row, SqlitePool};

use crate::models::recovery::RecoveryHistoryRecord;

/// Keep only the most recent resolutions so the log cannot grow unboundedly.
const MAX_RECOVERY_RECORDS: i64 = 200;

pub async fn insert_recovery_record(
    pool: &SqlitePool,
    record: &RecoveryHistoryRecord,
) -> Result<(), String> {
    let mut tx = crate::db::begin_immediate(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        r#"
        INSERT INTO recovery_history (
            id, task_id, task_file_name, action, source, error_code, created_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&record.id)
    .bind(&record.task_id)
    .bind(&record.task_file_name)
    .bind(&record.action)
    .bind(&record.source)
    .bind(&record.error_code)
    .bind(&record.created_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // Bound the log: delete everything beyond the newest MAX_RECOVERY_RECORDS.
    sqlx::query(
        r#"
        DELETE FROM recovery_history WHERE id IN (
            SELECT id FROM recovery_history
            ORDER BY created_at DESC, id DESC
            LIMIT -1 OFFSET ?
        )
        "#,
    )
    .bind(MAX_RECOVERY_RECORDS)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn list_recovery_history(
    pool: &SqlitePool,
    limit: u32,
) -> Result<Vec<RecoveryHistoryRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, task_file_name, action, source, error_code, created_at
        FROM recovery_history
        ORDER BY created_at DESC, id DESC
        LIMIT ?
        "#,
    )
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(recovery_record_from_row).collect())
}

fn recovery_record_from_row(row: sqlx::sqlite::SqliteRow) -> RecoveryHistoryRecord {
    RecoveryHistoryRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        task_file_name: row.get("task_file_name"),
        action: row.get("action"),
        source: row.get("source"),
        error_code: row.get("error_code"),
        created_at: row.get("created_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pool_name(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "vibe-recovery-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ))
    }

    fn record(id: &str, created_at: &str) -> RecoveryHistoryRecord {
        RecoveryHistoryRecord {
            id: id.to_string(),
            task_id: format!("task-{id}"),
            task_file_name: Some("file.bin".to_string()),
            action: "retry".to_string(),
            source: "recovery_center".to_string(),
            error_code: Some("http_status".to_string()),
            created_at: created_at.to_string(),
        }
    }

    #[tokio::test]
    async fn inserts_and_lists_newest_first() {
        let db_path = test_pool_name("list");
        let pool = crate::db::connect(&db_path).await.expect("test pool").pool;

        insert_recovery_record(&pool, &record("a", "2026-09-13T10:00:00Z"))
            .await
            .expect("insert a");
        insert_recovery_record(&pool, &record("b", "2026-09-13T11:00:00Z"))
            .await
            .expect("insert b");

        let history = list_recovery_history(&pool, 10).await.expect("list");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].id, "b", "newest first");
        assert_eq!(history[0].action, "retry");
        assert_eq!(history[0].source, "recovery_center");
        assert_eq!(history[0].task_file_name.as_deref(), Some("file.bin"));
    }

    #[tokio::test]
    async fn log_is_bounded_by_prune_on_insert() {
        let db_path = test_pool_name("bound");
        let pool = crate::db::connect(&db_path).await.expect("test pool").pool;

        for index in 0..(MAX_RECOVERY_RECORDS + 20) {
            // Two-digit timestamps would order wrongly past 100; pad to keep
            // lexical order aligned with insertion order.
            let stamp = format!("2026-09-13T10:{:04}", index);
            insert_recovery_record(&pool, &record(&index.to_string(), &stamp))
                .await
                .expect("insert");
        }

        let history = list_recovery_history(&pool, 1000).await.expect("list");
        assert_eq!(history.len() as i64, MAX_RECOVERY_RECORDS);
        // The oldest rows were pruned; the newest insert survives.
        assert_eq!(history[0].id, (MAX_RECOVERY_RECORDS + 19).to_string());
    }
}
