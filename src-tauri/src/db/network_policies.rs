//! Network grants are independent of site keys and credentials. Legacy and
//! restored tasks do not implicitly acquire access to private services.

use crate::download::network_policy::{
    authority, blocked, NetworkAuthorizationDraft, NetworkPolicy, TaskSource,
};
use sqlx::SqlitePool;

pub async fn task_network_policy(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<NetworkPolicy, String> {
    let raw: Option<String> =
        sqlx::query_scalar("SELECT policy_json FROM task_network_policies WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    match raw {
        Some(raw) => serde_json::from_str(&raw)
            .map_err(|_| blocked("Stored network authorization is invalid")),
        None => Ok(NetworkPolicy::public(TaskSource::Unknown, "")),
    }
}

pub async fn save_task_network_policy<'e, E>(
    executor: E,
    task_id: &str,
    policy: &NetworkPolicy,
) -> Result<(), String>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let policy_json = serde_json::to_string(policy).map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO task_network_policies (task_id, policy_json, updated_at) VALUES (?, ?, ?) ON CONFLICT(task_id) DO UPDATE SET policy_json=excluded.policy_json, updated_at=excluded.updated_at")
        .bind(task_id).bind(policy_json).bind(crate::models::task::now_iso()).execute(executor).await.map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn create_network_authorization(
    pool: &SqlitePool,
    source: TaskSource,
    url: &str,
) -> Result<NetworkAuthorizationDraft, String> {
    let policy = NetworkPolicy::confirm_target(source, url).await?;
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("DELETE FROM network_authorizations WHERE expires_at < ?")
        .bind(crate::models::task::now_iso())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    sqlx::query(
        "INSERT INTO network_authorizations (id, policy_json, expires_at) VALUES (?, ?, ?)",
    )
    .bind(&id)
    .bind(serde_json::to_string(&policy).map_err(|e| e.to_string())?)
    .bind(&expires_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(NetworkAuthorizationDraft {
        id,
        policy,
        expires_at,
    })
}

/// Reuse an existing task-bound policy for a new task without widening its
/// authority. The short-lived row is only a handoff token for creation; the
/// created task stores the policy itself.
pub async fn create_network_authorization_for_policy(
    pool: &SqlitePool,
    policy: &NetworkPolicy,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    sqlx::query(
        "INSERT INTO network_authorizations (id, policy_json, expires_at) VALUES (?, ?, ?)",
    )
    .bind(&id)
    .bind(serde_json::to_string(policy).map_err(|e| e.to_string())?)
    .bind(&expires_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub async fn revoke_task_network_policy(pool: &SqlitePool, task_id: &str) -> Result<(), String> {
    sqlx::query("INSERT INTO task_network_policies (task_id, policy_json, updated_at) VALUES (?, ?, ?) ON CONFLICT(task_id) DO UPDATE SET policy_json=excluded.policy_json, updated_at=excluded.updated_at")
        .bind(task_id)
        .bind(serde_json::to_string(&NetworkPolicy::public(TaskSource::Unknown, ""))
            .map_err(|e| e.to_string())?)
        .bind(crate::models::task::now_iso())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn draft_network_policy(
    pool: &SqlitePool,
    source: TaskSource,
    url: &str,
    authorization_id: Option<&str>,
) -> Result<NetworkPolicy, String> {
    let Some(id) = authorization_id else {
        return Ok(NetworkPolicy::public(source, url));
    };
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT policy_json FROM network_authorizations WHERE id = ? AND expires_at >= ?",
    )
    .bind(id)
    .bind(crate::models::task::now_iso())
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let policy: NetworkPolicy = raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .ok_or_else(|| blocked("Network authorization expired"))?;
    let target = authority(&reqwest::Url::parse(url).map_err(|e| blocked(e.to_string()))?)?;
    if policy.source != source || policy.root_authority.as_deref() != Some(target.as_str()) {
        return Err(blocked(
            "Network authorization belongs to a different target or source",
        ));
    }
    Ok(policy)
}
