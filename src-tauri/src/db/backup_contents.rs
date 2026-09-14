//! Content inventory for backups (feature proposal §3.7 "backup content
//! preview"). One counter serves both sides: the live database (export
//! preview via `describe_backup_source`) and a materialized backup snapshot
//! (restore preview inside `validate_app_backup`).

use sqlx::{Row, SqlitePool};

use crate::models::backup::BackupContents;

/// Settings row holding the browser capture rules, mirroring
/// `SETTING_BROWSER_CAPTURE` in `commands/browser.rs`. Declared locally to
/// keep this module free of command-layer dependencies.
const SETTING_BROWSER_CAPTURE: &str = "browser_capture_settings";

/// Count the backup-relevant rows of `pool`. All statements are static SQL
/// (sqlx injection audit) and aggregate-only, so the cost is one table scan
/// per counter on a database that is at most a few tens of MB.
pub async fn count_contents(pool: &SqlitePool) -> Result<BackupContents, String> {
    let mut contents = BackupContents::default();

    let count = |sql: &'static str| async move {
        let (value,): (i64,) = sqlx::query_as(sql)
            .fetch_one(pool)
            .await
            .map_err(|e| format!("Could not count backup contents: {e}"))?;
        Ok::<i64, String>(value)
    };

    contents.tasks_total = count("SELECT COUNT(*) FROM tasks").await? as u32;
    contents.tasks_completed =
        count("SELECT COUNT(*) FROM tasks WHERE status = 'completed'").await? as u32;
    contents.tasks_failed =
        count("SELECT COUNT(*) FROM tasks WHERE status IN ('failed', 'needs_attention')").await?
            as u32;
    contents.classification_rules =
        count("SELECT COUNT(*) FROM classification_rules").await? as u32;
    contents.tasks_with_checksums =
        count("SELECT COUNT(DISTINCT task_id) FROM task_checksums").await? as u32;
    contents.tasks_with_credentials =
        count("SELECT COUNT(DISTINCT task_id) FROM task_credentials").await? as u32;
    contents.tasks_with_request_headers =
        count("SELECT COUNT(DISTINCT task_id) FROM task_request_headers").await? as u32;
    contents.settings_keys = count("SELECT COUNT(*) FROM settings").await? as u32;
    contents.task_events = count("SELECT COUNT(*) FROM task_events").await? as u32;

    contents.site_rules = count_site_rules(pool).await?;
    Ok(contents)
}

/// Site rules live as a JSON array inside one settings row, so the rule count
/// is the array length (0 when the row is absent or malformed — a malformed
/// row must not fail the whole inventory).
async fn count_site_rules(pool: &SqlitePool) -> Result<u32, String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(SETTING_BROWSER_CAPTURE)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("Could not read capture settings: {e}"))?;
    let Some((value,)) = row else {
        return Ok(0);
    };
    let parsed: serde_json::Value = serde_json::from_str(&value).unwrap_or(serde_json::Value::Null);
    let len = parsed
        .get("siteRules")
        .and_then(serde_json::Value::as_array)
        .map(|rules| rules.len() as u32)
        .unwrap_or(0);
    Ok(len)
}

/// Distinct `save_dir` values in the tasks table, bounded by the caller.
pub async fn distinct_save_dirs(pool: &sqlx::SqlitePool) -> Result<Vec<String>, String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT save_dir FROM tasks ORDER BY save_dir")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("Could not list save dirs: {e}"))?;
    Ok(rows.into_iter().map(|(dir,)| dir).collect())
}

/// Count a single scalar from a row helper shared by the restore report.
pub async fn count_scalar(pool: &sqlx::SqlitePool, sql: &'static str) -> Result<u32, String> {
    let row = sqlx::query(sql)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("Could not run backup count: {e}"))?;
    Ok(row.try_get::<i64, _>(0).unwrap_or(0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .expect("settings table");
        pool
    }

    #[test]
    fn site_rules_count_parses_array_length() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let pool = memory_pool().await;
            for (value, expected) in [
                (r#"{"siteRules":[{"id":"a"},{"id":"b"}]}"#, 2u32),
                (r#"{}"#, 0),
                ("not-json", 0),
            ] {
                sqlx::query("INSERT OR REPLACE INTO settings(key, value) VALUES('browser_capture_settings', ?)")
                    .bind(value)
                    .execute(&pool)
                    .await
                    .expect("insert");
                assert_eq!(count_site_rules(&pool).await.unwrap(), expected);
            }
        });
    }

    #[test]
    fn missing_capture_row_counts_zero() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let pool = memory_pool().await;
            assert_eq!(count_site_rules(&pool).await.unwrap(), 0);
        });
    }
}
