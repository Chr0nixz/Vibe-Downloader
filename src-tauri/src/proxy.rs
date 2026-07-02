use std::{io, sync::Arc};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::{net::TcpStream, sync::RwLock};
use tokio_socks::tcp::Socks5Stream;

use crate::models::AppSettings;

const SERVICE: &str = "Vibe Downloader";
const ACCOUNT: &str = "proxy-password";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppProxyMode {
    Off,
    System,
    Custom,
}

impl AppProxyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::System => "system",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProxyConfig {
    pub mode: AppProxyMode,
    pub url: Option<String>,
    pub no_proxy: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

pub type SharedProxyConfig = Arc<RwLock<ResolvedProxyConfig>>;

impl Default for ResolvedProxyConfig {
    fn default() -> Self {
        Self {
            mode: AppProxyMode::Off,
            url: None,
            no_proxy: None,
            username: None,
            password: None,
        }
    }
}

impl ResolvedProxyConfig {
    pub fn from_settings(settings: &AppSettings) -> Self {
        let password = if settings.proxy_password_saved {
            load_proxy_password().ok().flatten()
        } else {
            None
        };
        Self {
            mode: settings.proxy_mode,
            url: normalize_proxy_url(&settings.proxy_url),
            no_proxy: normalize_proxy_no_proxy(&settings.proxy_no_proxy),
            username: normalize_proxy_optional(&settings.proxy_username),
            password,
        }
    }

    pub fn shared_default() -> SharedProxyConfig {
        Arc::new(RwLock::new(Self::default()))
    }

    pub fn is_custom_socks5(&self) -> bool {
        self.mode == AppProxyMode::Custom
            && self
                .url
                .as_deref()
                .and_then(|value| Url::parse(value).ok())
                .is_some_and(|url| url.scheme() == "socks5")
    }

    pub fn custom_socks5_url_with_auth(&self) -> Option<String> {
        if !self.is_custom_socks5() {
            return None;
        }
        let mut url = Url::parse(self.url.as_deref()?).ok()?;
        if let Some(username) = &self.username {
            let _ = url.set_username(username);
            let _ = url.set_password(Some(self.password.as_deref().unwrap_or("")));
        }
        Some(url.to_string())
    }

    pub fn fingerprint(&self) -> String {
        match self.mode {
            AppProxyMode::Off => "off".to_string(),
            AppProxyMode::System => "system".to_string(),
            AppProxyMode::Custom => format!(
                "custom:{}:{}:{}",
                self.url.as_deref().unwrap_or(""),
                self.no_proxy.as_deref().unwrap_or(""),
                self.username.as_deref().unwrap_or("")
            ),
        }
    }
}

pub fn normalize_proxy_mode(value: &str) -> AppProxyMode {
    match value.trim() {
        "system" => AppProxyMode::System,
        "custom" => AppProxyMode::Custom,
        _ => AppProxyMode::Off,
    }
}

pub fn normalize_proxy_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn normalize_proxy_no_proxy(value: &str) -> Option<String> {
    let entries = value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    (!entries.is_empty()).then(|| entries.join(","))
}

pub fn normalize_proxy_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Ok(url) = Url::parse(trimmed) else {
        return None;
    };
    if !matches!(url.scheme(), "http" | "https" | "socks5") {
        return None;
    }
    if url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    Some(url.to_string())
}

pub fn validate_proxy_settings(
    mode: AppProxyMode,
    proxy_url: &str,
    proxy_username: &str,
) -> Result<(), String> {
    if mode != AppProxyMode::Custom {
        return Ok(());
    }
    let url = normalize_proxy_url(proxy_url).ok_or_else(|| {
        "Proxy URL must be http://, https://, or socks5:// without embedded credentials."
            .to_string()
    })?;
    if normalize_proxy_optional(proxy_username).is_some() && Url::parse(&url).is_err() {
        return Err("Proxy URL is invalid.".to_string());
    }
    Ok(())
}

pub fn save_proxy_password(password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("OS key store is unavailable: {e}"))?;
    entry
        .set_password(password)
        .map_err(|e| format!("Could not save proxy password: {e}"))
}

pub fn load_proxy_password() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("OS key store is unavailable: {e}"))?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Could not load proxy password: {error}")),
    }
}

pub fn clear_proxy_password() -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("OS key store is unavailable: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("Could not clear proxy password: {error}")),
    }
}

pub async fn socks5_connect(
    proxy_url: &str,
    username: Option<&str>,
    password: Option<&str>,
    target_host: &str,
    target_port: u16,
) -> io::Result<TcpStream> {
    let proxy = Url::parse(proxy_url).map_err(invalid_input)?;
    if proxy.scheme() != "socks5" {
        return Err(invalid_input("Proxy URL must use socks5://."));
    }
    let proxy_host = proxy
        .host_str()
        .ok_or_else(|| invalid_input("Proxy URL is missing a host."))?;
    let proxy_port = proxy
        .port()
        .ok_or_else(|| invalid_input("Proxy URL is missing a port."))?;
    let username = username.filter(|value| !value.is_empty());
    let password = password.filter(|value| !value.is_empty());
    let target = format!("{target_host}:{target_port}");

    let stream = match (username, password) {
        (Some(user), Some(pass)) => {
            Socks5Stream::connect_with_password(
                (proxy_host, proxy_port),
                target.as_str(),
                user,
                pass,
            )
            .await
        }
        _ => Socks5Stream::connect((proxy_host, proxy_port), target.as_str()).await,
    }
    .map_err(map_socks5_error)?;

    Ok(stream.into_inner())
}

fn map_socks5_error(error: tokio_socks::Error) -> io::Error {
    use tokio_socks::Error as SocksError;
    let kind = match &error {
        SocksError::AuthorizationRequired
        | SocksError::NoAcceptableAuthMethods
        | SocksError::PasswordAuthFailure(_) => io::ErrorKind::PermissionDenied,
        SocksError::ConnectionRefused
        | SocksError::ConnectionNotAllowedByRuleset
        | SocksError::NetworkUnreachable
        | SocksError::HostUnreachable
        | SocksError::TtlExpired => io::ErrorKind::ConnectionRefused,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.to_string())
}

fn invalid_input(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}
