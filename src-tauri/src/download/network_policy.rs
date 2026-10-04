//! Task-bound network permissions. A grant authorizes one authority and the
//! addresses confirmed by the user, never every resource referenced by it.

use crate::models::AppErrorPayload;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskSource {
    #[default]
    Unknown,
    Manual,
    Clipboard,
    Browser,
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkTargetGrant {
    pub authority: String,
    pub addresses: Vec<String>,
    pub authorized_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicy {
    pub source: TaskSource,
    pub root_authority: Option<String>,
    pub grants: Vec<NetworkTargetGrant>,
    #[serde(skip)]
    #[specta(skip)]
    legacy_test_compat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NetworkAuthorizationDraft {
    pub id: String,
    pub policy: NetworkPolicy,
    pub expires_at: String,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            source: TaskSource::Unknown,
            root_authority: None,
            grants: Vec::new(),
            legacy_test_compat: true,
        }
    }
}

pub fn authority(url: &Url) -> Result<String, String> {
    let scheme = match url.scheme() {
        "webdav" => "http",
        "webdavs" => "https",
        scheme => scheme,
    };
    let host = url
        .host_str()
        .ok_or_else(|| blocked("Missing network host"))?;
    let port = url
        .port()
        .or(match scheme {
            "http" => Some(80),
            "https" => Some(443),
            "ftp" => Some(21),
            // FTPS without an explicit port is implicit TLS on 990. Explicit
            // port 21 remains available for AUTH TLS/explicit FTPS.
            "ftps" => Some(990),
            "sftp" => Some(22),
            _ => None,
        })
        .ok_or_else(|| blocked("Unsupported network authority"))?;
    Ok(format_authority_url(scheme, host, port))
}

/// Format an authority without losing IPv6 bracket syntax. URL parsers expose
/// IPv6 hosts inconsistently across versions (`::1` vs `[::1]`), while the
/// serialized authority must always use brackets before appending a port.
pub(crate) fn format_authority_url(scheme: &str, host: &str, port: u16) -> String {
    let host = host.trim();
    let host = if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{scheme}://{host}:{port}")
}

fn normalized_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}

/// Link-local and metadata endpoints stay forbidden even after confirmation.
/// NAS permission must never authorize a cloud credential service.
pub fn is_forbidden_ip(ip: &IpAddr) -> bool {
    match normalized_ip(*ip) {
        IpAddr::V4(ip) => {
            ip.is_unspecified()
                || ip.octets()[0] == 0
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_multicast()
                || ip.is_documentation()
                || ip.octets()[0] >= 240
        }
        IpAddr::V6(ip) => {
            ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unicast_link_local()
                // AWS exposes an IPv6 metadata endpoint inside this otherwise
                // user-authorizable ULA prefix.
                || ip.segments()[0] == 0xfd00 && ip.segments()[1] == 0x0ec2
        }
    }
}

pub fn blocked(detail: impl Into<String>) -> String {
    AppErrorPayload::new("intranet_target_blocked", detail, false, vec!["check_url"])
        .command_error()
}

impl NetworkPolicy {
    pub fn public(source: TaskSource, url: &str) -> Self {
        Self {
            source,
            root_authority: Url::parse(url).ok().and_then(|url| authority(&url).ok()),
            grants: Vec::new(),
            legacy_test_compat: false,
        }
    }

    pub fn fingerprint(&self) -> String {
        format!(
            "{}:{}",
            serde_json::to_string(self).unwrap_or_default(),
            self.legacy_test_compat
        )
    }

    fn test_bypassed(&self) -> bool {
        self.legacy_test_compat && super::ssrf::intranet_guard_bypassed()
    }

    fn authority_granted(&self, url: &Url) -> bool {
        authority(url)
            .ok()
            .is_some_and(|target| self.grants.iter().any(|grant| grant.authority == target))
    }

    pub fn allows_ip(&self, host: &str, ip: IpAddr) -> bool {
        let ip = normalized_ip(ip);
        if is_forbidden_ip(&ip) {
            return false;
        }
        if !super::ssrf::is_private_ip(&ip) || self.test_bypassed() {
            return true;
        }
        self.grants.iter().any(|grant| {
            Url::parse(&grant.authority)
                .ok()
                .is_some_and(|url| url.host_str() == Some(host))
                && grant
                    .addresses
                    .iter()
                    .any(|allowed| allowed.parse::<IpAddr>().ok().map(normalized_ip) == Some(ip))
        })
    }

    pub fn assert_literal(&self, url: &Url) -> Result<(), String> {
        let host = url
            .host_str()
            .ok_or_else(|| blocked("Missing network host"))?;
        if let Ok(ip) = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
        {
            self.assert_address(url, ip)?;
        } else if host.eq_ignore_ascii_case("localhost")
            && !self.authority_granted(url)
            && !self.test_bypassed()
        {
            return Err(blocked("Localhost requires explicit task authorization"));
        }
        Ok(())
    }

    pub fn assert_address(&self, url: &Url, ip: IpAddr) -> Result<(), String> {
        let ip = normalized_ip(ip);
        if is_forbidden_ip(&ip)
            || !self.allows_ip(url.host_str().unwrap_or_default(), ip)
            || (super::ssrf::is_private_ip(&ip)
                && !self.authority_granted(url)
                && !self.test_bypassed())
        {
            return Err(blocked(format!(
                "SSRF guard: {} ({ip}) requires a matching task authorization",
                authority(url)?
            )));
        }
        Ok(())
    }

    /// Connectors must dial these addresses without resolving the host again.
    /// A second lookup after validation would recreate a DNS rebinding gap.
    pub async fn resolve(&self, url: &Url) -> Result<Vec<SocketAddr>, String> {
        self.assert_literal(url)?;
        let host = url
            .host_str()
            .ok_or_else(|| blocked("Missing network host"))?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = network_port(url)?;
        let addresses = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            tokio::time::timeout(
                Duration::from_secs(3),
                tokio::net::lookup_host((host, port)),
            )
            .await
            .map_err(|_| {
                AppErrorPayload::new("timeout", "DNS lookup timed out", true, vec!["retry"])
                    .command_error()
            })?
            .map_err(|e| {
                AppErrorPayload::new("dns_failure", e.to_string(), false, vec!["check_url"])
                    .command_error()
            })?
            .collect::<Vec<_>>()
        };
        if addresses.is_empty() {
            return Err(blocked("DNS returned no usable addresses"));
        }
        for address in &addresses {
            self.assert_address(url, address.ip())?;
        }
        Ok(addresses)
    }

    pub async fn confirm_target(source: TaskSource, input: &str) -> Result<Self, String> {
        let url = Url::parse(input).map_err(|e| blocked(e.to_string()))?;
        let target = authority(&url)?;
        let host = url
            .host_str()
            .ok_or_else(|| blocked("Missing network host"))?
            .trim_start_matches('[')
            .trim_end_matches(']');
        let port = network_port(&url)?;
        let mut ips = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![ip]
        } else {
            tokio::time::timeout(
                Duration::from_secs(3),
                tokio::net::lookup_host((host, port)),
            )
            .await
            .map_err(|_| blocked("DNS lookup timed out"))?
            .map_err(|e| blocked(e.to_string()))?
            .map(|addr| addr.ip())
            .collect()
        };
        ips.sort();
        ips.dedup();
        if ips.is_empty() || ips.len() > 32 || ips.iter().any(is_forbidden_ip) {
            return Err(blocked("This target cannot be authorized"));
        }
        Ok(Self {
            source,
            root_authority: Some(target.clone()),
            grants: vec![NetworkTargetGrant {
                authority: target,
                addresses: ips
                    .into_iter()
                    .map(|ip| normalized_ip(ip).to_string())
                    .collect(),
                authorized_at: crate::models::task::now_iso(),
            }],
            legacy_test_compat: false,
        })
    }
}

pub fn network_port(url: &Url) -> Result<u16, String> {
    url.port()
        .or_else(|| match url.scheme() {
            "http" | "webdav" => Some(80),
            "https" | "webdavs" => Some(443),
            "ftp" => Some(21),
            "ftps" => Some(990),
            "sftp" => Some(22),
            _ => None,
        })
        .ok_or_else(|| blocked("Unsupported network port"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn forbidden_metadata_and_link_local_addresses_never_authorize() {
        for raw in [
            "169.254.169.254",
            "224.0.0.1",
            "0.0.0.0",
            "fe80::1",
            "ff02::1",
            "fd00:ec2::1",
        ] {
            let ip: IpAddr = raw.parse().expect("valid address");
            assert!(is_forbidden_ip(&ip), "{raw} must stay forbidden");
        }
    }

    #[test]
    fn localhost_requires_task_authorization_but_can_be_granted() {
        let url = Url::parse("http://localhost/share").unwrap();
        let loopback: IpAddr = "127.0.0.1".parse().unwrap();
        let public_only = NetworkPolicy::public(TaskSource::Manual, url.as_str());
        assert!(public_only.assert_address(&url, loopback).is_err());

        let authorized = NetworkPolicy {
            source: TaskSource::Manual,
            root_authority: Some("http://localhost:80".to_string()),
            grants: vec![NetworkTargetGrant {
                authority: "http://localhost:80".to_string(),
                addresses: vec![loopback.to_string()],
                authorized_at: "now".to_string(),
            }],
            legacy_test_compat: false,
        };
        assert!(authorized.assert_address(&url, loopback).is_ok());
    }

    #[test]
    fn private_ranges_require_matching_authority_and_address() {
        let policy = NetworkPolicy {
            source: TaskSource::Manual,
            root_authority: Some("http://nas.local:80".to_string()),
            grants: vec![NetworkTargetGrant {
                authority: "http://nas.local:80".to_string(),
                addresses: vec!["192.168.1.10".to_string()],
                authorized_at: "now".to_string(),
            }],
            legacy_test_compat: false,
        };
        let nas = Url::parse("http://nas.local/share").unwrap();
        assert!(policy
            .assert_address(&nas, Ipv4Addr::new(192, 168, 1, 10).into())
            .is_ok());
        assert!(policy
            .assert_address(&nas, Ipv4Addr::new(192, 168, 1, 11).into())
            .is_err());
        let other = Url::parse("http://other.local/share").unwrap();
        assert!(policy
            .assert_address(&other, Ipv4Addr::new(192, 168, 1, 10).into())
            .is_err());
    }

    #[test]
    fn ula_and_cgnat_are_private_but_public_is_not() {
        assert!(super::super::ssrf::is_private_ip(
            &"100.64.0.1".parse().unwrap()
        ));
        assert!(super::super::ssrf::is_private_ip(
            &"fd12:3456::1".parse().unwrap()
        ));
        assert!(!super::super::ssrf::is_private_ip(
            &"100.128.0.1".parse().unwrap()
        ));
        assert!(!super::super::ssrf::is_private_ip(
            &"2001:4860:4860::8888".parse().unwrap()
        ));
    }

    #[test]
    fn authority_preserves_ipv6_and_ftps_default_port() {
        let ipv6 = Url::parse("http://[2001:db8::10]/file.bin").unwrap();
        assert_eq!(authority(&ipv6).unwrap(), "http://[2001:db8::10]:80");

        let implicit_ftps = Url::parse("ftps://[2001:db8::10]/file.bin").unwrap();
        assert_eq!(
            authority(&implicit_ftps).unwrap(),
            "ftps://[2001:db8::10]:990"
        );
        assert_eq!(network_port(&implicit_ftps).unwrap(), 990);

        let explicit_ftps = Url::parse("ftps://[2001:db8::10]:21/file.bin").unwrap();
        assert_eq!(
            authority(&explicit_ftps).unwrap(),
            "ftps://[2001:db8::10]:21"
        );
        assert_eq!(network_port(&explicit_ftps).unwrap(), 21);
    }
}
