use std::{io, sync::Arc};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::RwLock,
};

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
    let mut stream = TcpStream::connect((proxy_host, proxy_port)).await?;

    if username.is_some() || password.is_some() {
        stream.write_all(&[0x05, 0x01, 0x02]).await?;
    } else {
        stream.write_all(&[0x05, 0x01, 0x00]).await?;
    }
    let mut method = [0_u8; 2];
    stream.read_exact(&mut method).await?;
    if method[0] != 0x05 {
        return Err(invalid_input("SOCKS5 proxy returned an invalid greeting."));
    }
    match method[1] {
        0x00 => {}
        0x02 => {
            authenticate_socks5(&mut stream, username.unwrap_or(""), password.unwrap_or("")).await?
        }
        0xff => {
            return Err(invalid_input(
                "SOCKS5 proxy did not accept the offered authentication method.",
            ));
        }
        _ => {
            return Err(invalid_input(
                "SOCKS5 proxy selected an unsupported method.",
            ))
        }
    }

    let host_bytes = target_host.as_bytes();
    if host_bytes.is_empty() || host_bytes.len() > u8::MAX as usize {
        return Err(invalid_input("SOCKS5 target host is invalid."));
    }
    let mut request = Vec::with_capacity(host_bytes.len() + 7);
    request.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&target_port.to_be_bytes());
    stream.write_all(&request).await?;

    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await?;
    if header[0] != 0x05 || header[1] != 0x00 {
        return Err(invalid_input(format!(
            "SOCKS5 proxy connection failed with status 0x{:02x}.",
            header[1]
        )));
    }
    match header[3] {
        0x01 => {
            let mut skip = [0_u8; 6];
            stream.read_exact(&mut skip).await?;
        }
        0x03 => {
            let mut len = [0_u8; 1];
            stream.read_exact(&mut len).await?;
            let mut skip = vec![0_u8; len[0] as usize + 2];
            stream.read_exact(&mut skip).await?;
        }
        0x04 => {
            let mut skip = [0_u8; 18];
            stream.read_exact(&mut skip).await?;
        }
        _ => {
            return Err(invalid_input(
                "SOCKS5 proxy returned an invalid address type.",
            ))
        }
    }
    Ok(stream)
}

async fn authenticate_socks5(
    stream: &mut TcpStream,
    username: &str,
    password: &str,
) -> io::Result<()> {
    if username.len() > u8::MAX as usize || password.len() > u8::MAX as usize {
        return Err(invalid_input("SOCKS5 username or password is too long."));
    }
    let mut request = Vec::with_capacity(username.len() + password.len() + 3);
    request.push(0x01);
    request.push(username.len() as u8);
    request.extend_from_slice(username.as_bytes());
    request.push(password.len() as u8);
    request.extend_from_slice(password.as_bytes());
    stream.write_all(&request).await?;
    let mut response = [0_u8; 2];
    stream.read_exact(&mut response).await?;
    if response != [0x01, 0x00] {
        return Err(invalid_input("SOCKS5 proxy authentication failed."));
    }
    Ok(())
}

fn invalid_input(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}
