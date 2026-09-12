use sqlx::{Row, SqlitePool};

use crate::models::BrowserKind;

pub async fn browser_message_exists(pool: &SqlitePool, request_id: &str) -> Result<bool, String> {
    let row = sqlx::query("SELECT 1 FROM browser_messages WHERE request_id = ?")
        .bind(request_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(row.is_some())
}

/// SEC-06: `browser_messages` is an unencrypted diagnostics table, so store a
/// query-stripped URL — pre-signed URLs and query tokens never land on disk.
/// Task creation itself consumes the full URL in memory before this point.
fn sanitize_url_for_storage(url: &str) -> String {
    match reqwest::Url::parse(url.trim()) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        // Unparseable input is not a URL leak vector; keep it verbatim.
        Err(_) => url.trim().to_string(),
    }
}

pub async fn insert_browser_message(
    pool: &SqlitePool,
    request_id: &str,
    browser: BrowserKind,
    url: &str,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), String> {
    let created_at = crate::models::task::now_iso();
    let url = sanitize_url_for_storage(url);

    sqlx::query(
        r#"
        INSERT INTO browser_messages (
            request_id, browser, url, status, error_message, created_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(request_id)
    .bind(browser.as_str())
    .bind(url)
    .bind(status)
    .bind(error_message)
    .bind(&created_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn update_browser_message_status(
    pool: &SqlitePool,
    request_id: &str,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE browser_messages
        SET status = ?, error_message = ?
        WHERE request_id = ?
        "#,
    )
    .bind(status)
    .bind(error_message)
    .bind(request_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn latest_browser_error(
    pool: &SqlitePool,
    browser: BrowserKind,
) -> Result<Option<String>, String> {
    let row = sqlx::query(
        r#"
        SELECT error_message FROM browser_messages
        WHERE browser = ? AND error_message IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(browser.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(row.map(|row| row.get("error_message")))
}

/// SEC-06: prune diagnostics rows — browser handoff records are transient
/// state, not an audit log. Same two-step shape as `prune_task_events`.
pub async fn prune_browser_messages(pool: &SqlitePool) -> Result<u64, String> {
    const MAX_AGE_DAYS: i64 = 30;
    const MAX_PER_BROWSER: i64 = 200;

    // RFC 3339 lexical ordering matches SQLite TEXT comparison because
    // `now_iso()` emits a fixed-offset UTC string (same pattern as
    // prune_request_diagnostics).
    let age_cutoff = (chrono::Utc::now() - chrono::Duration::days(MAX_AGE_DAYS)).to_rfc3339();
    let by_age = sqlx::query("DELETE FROM browser_messages WHERE created_at < ?")
        .bind(&age_cutoff)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();

    let by_cap = sqlx::query(
        r#"
        DELETE FROM browser_messages
        WHERE request_id IN (
            SELECT request_id FROM (
                SELECT request_id, ROW_NUMBER() OVER (
                    PARTITION BY browser ORDER BY created_at DESC
                ) AS rank
                FROM browser_messages
            ) WHERE rank > ?
        )
        "#,
    )
    .bind(MAX_PER_BROWSER)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();

    Ok(by_age + by_cap)
}
