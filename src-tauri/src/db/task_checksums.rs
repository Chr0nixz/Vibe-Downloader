use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::models::{
    ChecksumAlgorithm, HashVerificationStatus, TaskChecksumRecord,
};

pub async fn list_task_checksum_records(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<TaskChecksumRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, algorithm, expected_hash, actual_hash, status,
               source_kind, source_url, source_label, is_primary, weak,
               error_message, discovered_at, verified_at, created_at, updated_at
        FROM task_checksums
        WHERE task_id = ?
        ORDER BY is_primary DESC, algorithm ASC, created_at ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_checksum).collect()
}

pub async fn list_task_checksum_records_for_tasks(
    pool: &SqlitePool,
    task_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<TaskChecksumRecord>>, String> {
    if task_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT id, task_id, algorithm, expected_hash, actual_hash, status,
               source_kind, source_url, source_label, is_primary, weak,
               error_message, discovered_at, verified_at, created_at, updated_at
        FROM task_checksums
        WHERE task_id IN (
        "#,
    );
    let mut separated = query.separated(", ");
    for task_id in task_ids {
        separated.push_bind(task_id);
    }
    separated.push_unseparated(
        r#")
        ORDER BY task_id ASC, is_primary DESC, algorithm ASC, created_at ASC
        "#,
    );

    let rows = query
        .build()
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut checksums_by_task_id: std::collections::HashMap<String, Vec<TaskChecksumRecord>> =
        std::collections::HashMap::new();
    for row in rows.iter() {
        let record = row_to_checksum(row)?;
        checksums_by_task_id
            .entry(record.task_id.clone())
            .or_default()
            .push(record);
    }
    Ok(checksums_by_task_id)
}

pub async fn insert_task_checksum_record(
    pool: &SqlitePool,
    checksum: &TaskChecksumRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO task_checksums (
            id, task_id, algorithm, expected_hash, actual_hash, status,
            source_kind, source_url, source_label, is_primary, weak,
            error_message, discovered_at, verified_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&checksum.id)
    .bind(&checksum.task_id)
    .bind(checksum.algorithm.as_str())
    .bind(&checksum.expected_hash)
    .bind(&checksum.actual_hash)
    .bind(checksum.status.as_str())
    .bind(&checksum.source_kind)
    .bind(&checksum.source_url)
    .bind(&checksum.source_label)
    .bind(if checksum.is_primary { 1 } else { 0 })
    .bind(if checksum.weak { 1 } else { 0 })
    .bind(&checksum.error_message)
    .bind(&checksum.discovered_at)
    .bind(&checksum.verified_at)
    .bind(&checksum.created_at)
    .bind(&checksum.updated_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn row_to_checksum(row: &sqlx::sqlite::SqliteRow) -> Result<TaskChecksumRecord, String> {
    Ok(TaskChecksumRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        algorithm: ChecksumAlgorithm::from_db_str(row.get::<String, _>("algorithm").as_str()),
        expected_hash: row.get("expected_hash"),
        actual_hash: row.get("actual_hash"),
        status: HashVerificationStatus::from_db_str(row.get::<String, _>("status").as_str()),
        source_kind: row.get("source_kind"),
        source_url: row.get("source_url"),
        source_label: row.get("source_label"),
        is_primary: row.get::<i64, _>("is_primary") != 0,
        weak: row.get::<i64, _>("weak") != 0,
        error_message: row.get("error_message"),
        discovered_at: row.get("discovered_at"),
        verified_at: row.get("verified_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
