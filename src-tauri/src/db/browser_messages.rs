use sqlx::{Row, SqlitePool};

use crate::models::{BrowserHandoffRecord, BrowserKind};

/// Totals for the whole `browser_messages` table (not just the recent
/// window), so the integration center can show "recent N entries" and
/// cumulative counts without implying they cover the same rows.
pub struct BrowserMessageSummary {
    pub received_count: u32,
    pub failed_count: u32,
    pub last_created_at: Option<String>,
}

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

/// §3.9: recent handoff records for the integration center history panel.
/// The table is pruned (30 days / 200 per browser), so a missing row means
/// "not recorded or pruned", never "the handoff never happened". Rows whose
/// browser kind cannot be parsed (written by a newer build) are skipped
/// rather than surfaced as broken entries.
pub async fn recent_browser_messages(
    pool: &SqlitePool,
    limit: i64,
) -> Result<Vec<BrowserHandoffRecord>, String> {
    let rows = sqlx::query(
        r#"
        SELECT request_id, browser, url, status, error_message, created_at
        FROM browser_messages
        ORDER BY created_at DESC, request_id DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let browser_raw: String = row.try_get("browser").ok()?;
            let browser = BrowserKind::from_db_str(&browser_raw)?;
            Some(BrowserHandoffRecord {
                request_id: row.try_get("request_id").ok()?,
                browser,
                url: row.try_get("url").ok()?,
                status: row.try_get("status").ok()?,
                error_message: row.try_get("error_message").ok()?,
                created_at: row.try_get("created_at").ok()?,
            })
        })
        .collect())
}

/// `MAX(created_at)` is lexically correct here because `now_iso()` emits a
/// fixed-offset UTC string (same invariant as `prune_browser_messages`).
pub async fn browser_message_summary(pool: &SqlitePool) -> Result<BrowserMessageSummary, String> {
    let row = sqlx::query(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN status = 'received' THEN 1 ELSE 0 END), 0) AS received_count,
            COALESCE(SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), 0) AS failed_count,
            MAX(created_at) AS last_created_at
        FROM browser_messages
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(BrowserMessageSummary {
        received_count: row.try_get("received_count").map_err(|e| e.to_string())?,
        failed_count: row.try_get("failed_count").map_err(|e| e.to_string())?,
        last_created_at: row.try_get("last_created_at").map_err(|e| e.to_string())?,
    })
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
