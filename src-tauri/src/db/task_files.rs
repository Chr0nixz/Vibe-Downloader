use std::collections::HashMap;

use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::models::{TaskFileRecord, TaskStatus};

pub async fn insert_task_file_record(
    pool: &SqlitePool,
    file: &TaskFileRecord,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO task_files (
            id, task_id, relative_path, file_name, save_dir, temp_path, final_path,
            total_size, downloaded_bytes, selected, status, content_type
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&file.id)
    .bind(&file.task_id)
    .bind(&file.relative_path)
    .bind(&file.file_name)
    .bind(&file.save_dir)
    .bind(&file.temp_path)
    .bind(&file.final_path)
    .bind(file.total_size)
    .bind(file.downloaded_bytes)
    .bind(if file.selected { 1 } else { 0 })
    .bind(file.status.as_str())
    .bind(&file.content_type)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn list_task_file_records(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<TaskFileRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_id, relative_path, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, selected, status, content_type
        FROM task_files
        WHERE task_id = ?
        ORDER BY relative_path ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    rows.iter().map(row_to_task_file).collect()
}

pub async fn list_task_file_records_for_tasks(
    pool: &SqlitePool,
    task_ids: &[String],
) -> Result<HashMap<String, Vec<TaskFileRecord>>, String> {
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT id, task_id, relative_path, file_name, save_dir, temp_path, final_path,
               total_size, downloaded_bytes, selected, status, content_type
        FROM task_files
        WHERE task_id IN (
        "#,
    );
    let mut separated = query.separated(", ");
    for task_id in task_ids {
        separated.push_bind(task_id);
    }
    separated.push_unseparated(") ORDER BY task_id ASC, relative_path ASC");

    let rows = query
        .build()
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;

    let mut files_by_task_id: HashMap<String, Vec<TaskFileRecord>> = HashMap::new();
    for row in rows {
        let file = row_to_task_file(&row)?;
        files_by_task_id
            .entry(file.task_id.clone())
            .or_default()
            .push(file);
    }
    Ok(files_by_task_id)
}

fn row_to_task_file(row: &sqlx::sqlite::SqliteRow) -> Result<TaskFileRecord, String> {
    Ok(TaskFileRecord {
        id: row.get("id"),
        task_id: row.get("task_id"),
        relative_path: row.get("relative_path"),
        file_name: row.get("file_name"),
        save_dir: row.get("save_dir"),
        temp_path: row.get("temp_path"),
        final_path: row.get("final_path"),
        total_size: row.get("total_size"),
        downloaded_bytes: row.get("downloaded_bytes"),
        selected: row.get::<i64, _>("selected") != 0,
        status: TaskStatus::from_db_str(row.get::<String, _>("status").as_str()),
        content_type: row.get("content_type"),
    })
}
