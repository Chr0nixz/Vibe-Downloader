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

    bump_task_files_version(pool, &file.task_id).await?;
    Ok(())
}

pub async fn insert_task_file_record_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
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
    .execute(&mut **tx)
    .await
    .map_err(|e| e.to_string())?;

    bump_task_files_version_in_tx(tx, &file.task_id).await?;
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

pub async fn update_task_file_selection(
    pool: &SqlitePool,
    task_id: &str,
    selected_relative_paths: &[String],
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    update_task_file_selection_in_tx(&mut tx, task_id, selected_relative_paths).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn update_task_file_selection_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
    selected_relative_paths: &[String],
) -> Result<(), String> {
    sqlx::query("UPDATE task_files SET selected = 0 WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;

    for path in selected_relative_paths {
        sqlx::query("UPDATE task_files SET selected = 1 WHERE task_id = ? AND relative_path = ?")
            .bind(task_id)
            .bind(path)
            .execute(&mut **tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    bump_task_files_version_in_tx(tx, task_id).await
}

pub async fn update_task_file_progress(
    pool: &SqlitePool,
    file_id: &str,
    downloaded_bytes: i64,
    status: crate::models::TaskStatus,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE task_files
        SET downloaded_bytes = ?, status = ?
        WHERE id = ?
        "#,
    )
    .bind(downloaded_bytes.max(0))
    .bind(status.as_str())
    .bind(file_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn bump_task_files_version(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE tasks SET files_version = files_version + 1 WHERE id = ?")
        .bind(task_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn bump_task_files_version_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE tasks SET files_version = files_version + 1 WHERE id = ?")
        .bind(task_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{task::now_iso, TaskKind, TaskPriority, TaskRecord, TaskStatus};
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn insert_and_update_selection_in_transaction_bumps_files_version() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        init_task_tables(&pool).await;

        let task = sample_task("task-files-tx");
        super::super::insert_task_record(&pool, &task)
            .await
            .expect("insert task");
        let file = sample_file("task-files-tx", "a.bin", true);

        let mut tx = pool.begin().await.expect("begin");
        insert_task_file_record_in_tx(&mut tx, &file)
            .await
            .expect("insert file");
        update_task_file_selection_in_tx(&mut tx, "task-files-tx", &[String::from("a.bin")])
            .await
            .expect("update selection");
        tx.commit().await.expect("commit");

        let version: i64 = sqlx::query_scalar("SELECT files_version FROM tasks WHERE id = ?")
            .bind("task-files-tx")
            .fetch_one(&pool)
            .await
            .expect("version");
        assert_eq!(version, 2);
        let selected: i64 = sqlx::query_scalar(
            "SELECT selected FROM task_files WHERE task_id = ? AND relative_path = ?",
        )
        .bind("task-files-tx")
        .bind("a.bin")
        .fetch_one(&pool)
        .await
        .expect("selection");
        assert_eq!(selected, 1);
    }

    async fn init_task_tables(pool: &sqlx::SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE tasks (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                final_url TEXT,
                protocol TEXT NOT NULL,
                task_kind TEXT NOT NULL,
                file_name TEXT NOT NULL,
                save_dir TEXT NOT NULL,
                temp_path TEXT,
                final_path TEXT,
                total_size INTEGER NOT NULL,
                downloaded_bytes INTEGER NOT NULL,
                status TEXT NOT NULL,
                etag TEXT,
                last_modified TEXT,
                content_type TEXT,
                supports_resume INTEGER NOT NULL,
                supports_parallel INTEGER NOT NULL,
                supports_multi_file INTEGER NOT NULL,
                source_key TEXT NOT NULL,
                connection_count INTEGER NOT NULL,
                speed_bps INTEGER NOT NULL,
                task_speed_limit_bps TEXT,
                priority TEXT NOT NULL,
                queue_position INTEGER NOT NULL,
                category_key TEXT,
                obey_schedule INTEGER NOT NULL,
                health_summary TEXT,
                error_message TEXT,
                error_code TEXT,
                recovery_actions TEXT NOT NULL,
                retry_after_at TEXT,
                expected_hash_sha256 TEXT,
                actual_hash_sha256 TEXT,
                hash_status TEXT NOT NULL,
                hash_error TEXT,
                hash_verified_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                files_version INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("tasks table");
        sqlx::query(
            r#"
            CREATE TABLE task_files (
                id TEXT PRIMARY KEY,
                task_id TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                file_name TEXT NOT NULL,
                save_dir TEXT NOT NULL,
                temp_path TEXT,
                final_path TEXT,
                total_size INTEGER NOT NULL,
                downloaded_bytes INTEGER NOT NULL,
                selected INTEGER NOT NULL,
                status TEXT NOT NULL,
                content_type TEXT
            )
            "#,
        )
        .execute(pool)
        .await
        .expect("task_files table");
    }

    fn sample_task(task_id: &str) -> TaskRecord {
        let now = now_iso();
        TaskRecord {
            id: task_id.to_string(),
            url: "http://example.com/file".to_string(),
            final_url: Some("http://example.com/file".to_string()),
            protocol: "http".to_string(),
            task_kind: TaskKind::SingleFile,
            file_name: "file.bin".to_string(),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: Some(std::env::temp_dir().join("file.bin.vibe-downloading").to_string_lossy().to_string()),
            final_path: Some(std::env::temp_dir().join("file.bin").to_string_lossy().to_string()),
            total_size: 100,
            downloaded_bytes: 0,
            status: TaskStatus::Queued,
            etag: None,
            last_modified: None,
            content_type: None,
            supports_resume: true,
            supports_parallel: true,
            supports_multi_file: false,
            source_key: "http://example.com/file".to_string(),
            connection_count: 0,
            speed_bps: 0,
            task_speed_limit_bps: None,
            priority: TaskPriority::Normal,
            queue_position: 0,
            category_key: None,
            obey_schedule: true,
            health_summary: None,
            error_message: None,
            error_code: None,
            recovery_actions: Vec::new(),
            retry_after_at: None,
            expected_hash_sha256: None,
            actual_hash_sha256: None,
            hash_status: crate::models::HashVerificationStatus::NotRequested,
            hash_error: None,
            hash_verified_at: None,
            created_at: now.clone(),
            updated_at: now,
            files_version: 0,
        }
    }

    fn sample_file(task_id: &str, relative_path: &str, selected: bool) -> TaskFileRecord {
        TaskFileRecord {
            id: format!("{task_id}-{relative_path}"),
            task_id: task_id.to_string(),
            relative_path: relative_path.to_string(),
            file_name: relative_path.to_string(),
            save_dir: std::env::temp_dir().to_string_lossy().to_string(),
            temp_path: None,
            final_path: None,
            total_size: 100,
            downloaded_bytes: 0,
            selected,
            status: TaskStatus::Queued,
            content_type: None,
        }
    }
}
