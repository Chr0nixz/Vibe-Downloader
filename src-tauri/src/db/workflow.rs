//! Library validation and atomic field patches; omitted fields never clear data.
use sqlx::{Row, SqlitePool};

use crate::models::{
    is_sensitive_request_header, normalize_task_request_profile, AppErrorPayload,
    BulkTaskOptionsPatch, TaskProxyMode, WorkflowData, WorkflowEntry, WorkflowLibrary,
};

pub fn invalid_workflow() -> String {
    AppErrorPayload::new(
        "workflow_invalid",
        "Invalid reusable configuration.",
        false,
        vec![],
    )
    .command_error()
}

fn valid_text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.chars().any(char::is_control)
}

pub fn validate_entry(entry: &WorkflowEntry) -> Result<(), String> {
    if uuid::Uuid::parse_str(&entry.id).is_err()
        || entry.name.trim().is_empty()
        || !valid_text(&entry.name, 240)
    {
        return Err(invalid_workflow());
    }
    match &entry.data {
        WorkflowData::TaskPreset(preset) => {
            if preset.save_dir.trim().is_empty()
                || !valid_text(&preset.save_dir, 4096)
                || preset
                    .category_key
                    .as_deref()
                    .is_some_and(|v| !valid_text(v, 256))
                || preset
                    .task_speed_limit_bps
                    .as_deref()
                    .is_some_and(|v| super::normalize_speed_limit_bps(v).is_none())
                || preset
                    .proxy_no_proxy
                    .as_deref()
                    .is_some_and(|v| !valid_text(v, 4096))
            {
                return Err(invalid_workflow());
            }
            if preset.proxy_mode == TaskProxyMode::Custom {
                let url = reqwest::Url::parse(preset.proxy_url.as_deref().unwrap_or(""))
                    .map_err(|_| invalid_workflow())?;
                if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !matches!(url.path(), "" | "/")
                {
                    return Err(invalid_workflow());
                }
            } else if preset.proxy_url.is_some() || preset.proxy_no_proxy.is_some() {
                return Err(invalid_workflow());
            }
        }
        WorkflowData::SiteProfile(site) => {
            let url = reqwest::Url::parse(&site.origin).map_err(|_| invalid_workflow())?;
            if !matches!(url.scheme(), "http" | "https")
                || url.origin().ascii_serialization() != site.origin
            {
                return Err(invalid_workflow());
            }
            let headers = normalize_task_request_profile(&site.origin, &site.profile)?;
            if headers.is_empty()
                || headers
                    .iter()
                    .any(|(name, _)| is_sensitive_request_header(name))
            {
                return Err(invalid_workflow());
            }
            if let Some(referer) = &site.profile.referer {
                let referer = reqwest::Url::parse(referer).map_err(|_| invalid_workflow())?;
                if referer.query().is_some() || referer.fragment().is_some() {
                    return Err(invalid_workflow());
                }
            }
        }
        WorkflowData::SavedView(view) => {
            if ![
                "all",
                "downloading",
                "queue",
                "issues",
                "attention",
                "paused",
                "completed",
                "failed",
            ]
            .contains(&view.nav.as_str())
                || ![
                    "updated_at",
                    "created_at",
                    "file_size",
                    "progress",
                    "speed",
                    "status",
                    "queue_order",
                ]
                .contains(&view.sort_key.as_str())
                || !["asc", "desc"].contains(&view.sort_direction.as_str())
                || ![
                    "all", "archive", "image", "video", "document", "app", "other",
                ]
                .contains(&view.file_type.as_str())
                || !["all", "resumable", "single_connection"].contains(&view.resume.as_str())
                || ![
                    "all",
                    "none",
                    "http",
                    "network",
                    "auth",
                    "disk_write",
                    "remote_changed",
                    "temp_file",
                    "resume_unavailable",
                    "proxy",
                    "schedule",
                    "hls",
                    "dash",
                    "metalink",
                    "bt",
                    "ftp",
                    "sftp",
                    "webdav",
                    "other",
                ]
                .contains(&view.failure.as_str())
                || !valid_text(&view.search, 2048)
                || !valid_text(&view.source, 512)
            {
                return Err(invalid_workflow());
            }
        }
    }
    Ok(())
}

pub async fn list(pool: &SqlitePool) -> Result<WorkflowLibrary, String> {
    let rows = sqlx::query("SELECT id, name, format_version, payload FROM workflow_library ORDER BY name, id LIMIT 301")
        .fetch_all(pool).await.map_err(|e| e.to_string())?;
    let mut library = WorkflowLibrary {
        entries: vec![],
        unsupported_count: 0,
    };
    for row in rows {
        let data = serde_json::from_str::<WorkflowData>(row.get::<String, _>("payload").as_str());
        if row.get::<i64, _>("format_version") != 1 || data.is_err() {
            library.unsupported_count += 1;
            continue;
        }
        let entry = WorkflowEntry {
            id: row.get("id"),
            name: row.get("name"),
            data: data.map_err(|_| invalid_workflow())?,
        };
        if validate_entry(&entry).is_ok() {
            library.entries.push(entry);
        } else {
            library.unsupported_count += 1;
        }
    }
    Ok(library)
}

pub async fn save(pool: &SqlitePool, entry: &WorkflowEntry) -> Result<(), String> {
    validate_entry(entry)?;
    let payload = serde_json::to_string(&entry.data).map_err(|_| invalid_workflow())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let mut tx = sqlx::Connection::begin_with(&mut *conn, "BEGIN IMMEDIATE")
        .await
        .map_err(|e| e.to_string())?;
    let version: Option<i64> =
        sqlx::query_scalar("SELECT format_version FROM workflow_library WHERE id=?")
            .bind(&entry.id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_library")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if version.is_some_and(|v| v != 1) || (version.is_none() && count >= 300) {
        return Err(invalid_workflow());
    }
    sqlx::query("INSERT INTO workflow_library(id,name,format_version,payload) VALUES(?,?,1,?) ON CONFLICT(id) DO UPDATE SET name=excluded.name,payload=excluded.payload")
        .bind(&entry.id).bind(entry.name.trim()).bind(payload).execute(&mut *tx).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<(), String> {
    sqlx::query("DELETE FROM workflow_library WHERE id=? AND format_version=1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn validate_patch(patch: &BulkTaskOptionsPatch) -> Result<(), String> {
    if (patch.clear_speed_limit && patch.task_speed_limit_bps.is_some())
        || (patch.clear_category && patch.category_key.is_some())
        || patch
            .task_speed_limit_bps
            .as_deref()
            .is_some_and(|v| super::normalize_speed_limit_bps(v).is_none())
        || patch
            .category_key
            .as_deref()
            .is_some_and(|v| v.trim().is_empty() || !valid_text(v, 256))
        || (patch.priority.is_none()
            && patch.task_speed_limit_bps.is_none()
            && !patch.clear_speed_limit
            && patch.category_key.is_none()
            && !patch.clear_category
            && patch.obey_schedule.is_none())
    {
        return Err(invalid_workflow());
    }
    Ok(())
}

pub async fn patch_task(
    pool: &SqlitePool,
    id: &str,
    patch: &BulkTaskOptionsPatch,
) -> Result<bool, String> {
    validate_patch(patch)?;
    // CASE preserves fields at the database write boundary, even when another
    // editor changes a field after the batch's preview was rendered.
    let result = sqlx::query("UPDATE tasks SET priority=COALESCE(?,priority), task_speed_limit_bps=CASE WHEN ? THEN NULL WHEN ? IS NOT NULL THEN ? ELSE task_speed_limit_bps END, category_key=CASE WHEN ? THEN NULL WHEN ? IS NOT NULL THEN ? ELSE category_key END, obey_schedule=COALESCE(?,obey_schedule), updated_at=? WHERE id=? AND status NOT IN ('completed','cancelled')")
        .bind(patch.priority.map(|p| p.as_str())).bind(patch.clear_speed_limit)
        .bind(&patch.task_speed_limit_bps).bind(&patch.task_speed_limit_bps)
        .bind(patch.clear_category).bind(&patch.category_key).bind(&patch.category_key)
        .bind(patch.obey_schedule).bind(crate::models::task::now_iso()).bind(id)
        .execute(pool).await.map_err(|e| e.to_string())?;
    Ok(result.rows_affected() > 0)
}
