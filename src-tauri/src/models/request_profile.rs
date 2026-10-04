//! Task request profiles keep HTTP overrides separate from browser handoff metadata.

use std::collections::HashSet;

use reqwest::header::{HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::AppErrorPayload;

pub const MAX_CUSTOM_HEADERS: usize = 16;
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequestHeaderInput {
    pub name: String,
    pub value: String,
}

// Values can contain session tokens; IPC inputs must never print them via Debug.
#[derive(Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequestProfileInput {
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub custom_headers: Vec<TaskRequestHeaderInput>,
}

impl std::fmt::Debug for TaskRequestProfileInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskRequestProfileInput")
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskRequestProfileView {
    pub task_id: String,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub custom_headers: Vec<TaskRequestHeaderInput>,
    pub sensitive_header_names: Vec<String>,
    pub sensitive_expires_at: Option<String>,
    pub sensitive_expired: bool,
}

impl std::fmt::Debug for TaskRequestHeaderInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskRequestHeaderInput")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Custom X-* fields may carry API tokens, so all of them share Cookie
/// expiry and origin restrictions even when a token name is unfamiliar.
pub fn is_sensitive_request_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("x-")
        || matches!(
            name.as_str(),
            "authorization" | "cookie" | "cookie2" | "proxy-authorization" | "www-authenticate"
        )
}

fn invalid_profile() -> String {
    AppErrorPayload::new(
        "request_profile_invalid",
        "Invalid task request profile.",
        false,
        vec![],
    )
    .command_error()
}

pub fn normalize_task_request_profile(
    url: &str,
    input: &TaskRequestProfileInput,
) -> Result<Vec<(String, String)>, String> {
    let mut headers = Vec::new();
    let mut names = HashSet::new();
    let mut bytes = 0;
    let mut add = |name: &str, value: &str| -> Result<(), String> {
        // Check before trimming: a trailing newline is still an injection attempt.
        if name.bytes().any(|b| b < 0x20 || b == 0x7f)
            || name.len() > 128
            || value.bytes().any(|b| b < 0x20 || b == 0x7f)
            || value.len() > 8192
        {
            return Err(invalid_profile());
        }
        let value = value.trim();
        if value.is_empty() || HeaderValue::from_str(value).is_err() {
            return Err(invalid_profile());
        }
        let name = name.trim().to_ascii_lowercase();
        if name.len() > 128
            || HeaderName::from_bytes(name.as_bytes()).is_err()
            || !names.insert(name.clone())
        {
            return Err(invalid_profile());
        }
        bytes += name.len() + value.len();
        if bytes > MAX_HEADER_BYTES {
            return Err(invalid_profile());
        }
        headers.push((name, value.to_string()));
        Ok(())
    };
    if let Some(value) = input.user_agent.as_deref().filter(|v| !v.is_empty()) {
        add("user-agent", value)?;
    }
    if let Some(value) = input.referer.as_deref().filter(|v| !v.is_empty()) {
        let parsed = reqwest::Url::parse(value.trim()).map_err(|_| invalid_profile())?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid_profile());
        }
        add("referer", value)?;
    }
    if input.custom_headers.len() > MAX_CUSTOM_HEADERS {
        return Err(invalid_profile());
    }
    for header in &input.custom_headers {
        let name = header.name.trim().to_ascii_lowercase();
        // Framing, routing, proxy and browser-owned fields must remain engine-owned.
        let allowed = matches!(
            name.as_str(),
            "accept" | "accept-language" | "origin" | "dnt" | "cache-control" | "pragma" | "cookie"
        ) || (name.starts_with("x-")
            && !name.starts_with("x-forwarded-")
            && !name.starts_with("x-proxy-")
            && name != "x-real-ip");
        if !allowed {
            return Err(invalid_profile());
        }
        add(&header.name, &header.value)?;
    }
    if !headers.is_empty() {
        let parsed = reqwest::Url::parse(url).map_err(|_| invalid_profile())?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || crate::download::url_classify::is_torrent_url(&parsed)
        {
            return Err(AppErrorPayload::new(
                "request_profile_unsupported",
                "Task request profiles require HTTP or HTTPS.",
                false,
                vec![],
            )
            .command_error());
        }
    }
    Ok(headers)
}
