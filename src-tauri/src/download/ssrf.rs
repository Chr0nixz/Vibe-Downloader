//! A-2: SSRF (Server-Side Request Forgery) defense primitives shared by the
//! browser handoff path (`commands::browser`) and the HTTP download engine
//! (`download::http`).
//!
//! Prior to A-2 the SSRF guard lived only in `commands::browser` and checked
//! the literal hostname string once at handoff time. That left two bypass
//! vectors:
//!
//! 1. **DNS rebinding** — a public hostname can resolve to a private IP
//!    (`127.0.0.1`, `169.254.169.254`, `10.x`) at connection time, after the
//!    handoff URL check has passed.
//! 2. **Redirect to intranet** — reqwest followed up to 10 redirects without
//!    re-checking any hop's target IP, so a public URL could 302 to an
//!    internal address.
//!
//! This module centralizes the IP classification so both the handoff
//! pre-flight DNS check and the HTTP client's connection-time / redirect-time
//! guards use the same source of truth.

use std::net::IpAddr;

use reqwest::Url;

/// Returns `true` if `ip` is private, reserved, or otherwise unsuitable as a
/// target for browser-handoff-initiated or engine-initiated outbound requests
/// when the "allow intranet" toggle is off.
///
/// Covers all address classes the prior implementation missed:
/// - IPv4: loopback, RFC1918 private, link-local, unspecified, broadcast,
///   documentation, CGNAT (`100.64.0.0/10` via `is_shared`), `0.0.0.0/8`.
/// - IPv6: loopback, unspecified, multicast, unique-local (`fc00::/7`),
///   link-local (`fe80::/10`), plus IPv4-mapped IPv6 (`::ffff:a.b.c.d`)
///   which is first reduced to its embedded IPv4 form so the IPv4 rules
///   apply uniformly.
pub fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                // CGNAT 100.64.0.0/10 (RFC 6598) — `is_shared()` is unstable
                // on `std::net::Ipv4Addr`, so check the prefix manually:
                // first octet == 100, second octet in 64..=127.
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                || octets[0] == 0 // 0.0.0.0/8 "this network"
        }
        IpAddr::V6(v6) => {
            // IPv4-mapped IPv6 (::ffff:a.b.c.d) must be reduced to its IPv4
            // form before checking, otherwise the V6 checks below would miss
            // embedded loopback/private addresses.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_ip(&IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local() // fc00::/7 ULA
                || v6.is_unicast_link_local() // fe80::/10
        }
    }
}

/// Returns `true` if `url`'s host is a literal IP that is private/reserved,
/// or if the host is missing or the literal string "localhost".
///
/// This is the synchronous literal-IP check. For hostnames that require DNS
/// resolution, use [`is_hostname_private_via_dns`].
pub fn is_private_or_reserved_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    if host == "localhost" {
        return true;
    }
    // url::host_str() serializes IPv6 hosts inside brackets (e.g. "[::1]").
    // Strip them so parse::<IpAddr>() succeeds and loopback/private IPv6
    // addresses are caught by the SSRF guard.
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(&ip);
    }
    false
}

/// A-2: Pre-flight DNS check — resolves `host` and rejects if ANY resolved IP
/// is private or reserved. Closes the DNS rebinding gap where a public
/// hostname resolves to `127.0.0.1` / `169.254.169.254` / `10.x` at lookup
/// time, after the handoff URL literal check has passed.
///
/// - 3-second timeout to avoid blocking the handoff path on slow DNS.
/// - Returns `false` on resolution failure so handoff stays consistent with
///   the existing tolerant behavior (literal-IP check also only rejects known
///   private IPs; an unparseable host is allowed to proceed and fail later at
///   the engine layer).
pub async fn is_hostname_private_via_dns(host: &str) -> bool {
    let timeout = std::time::Duration::from_secs(3);
    let lookup = tokio::time::timeout(
        timeout,
        tokio::net::lookup_host(format!("{host}:0")),
    )
    .await;
    match lookup {
        Ok(Ok(mut iter)) => iter.any(|addr| is_private_ip(&addr.ip())),
        // Timeout or resolution error: do not block — the connection-time
        // resolver guard (`HickoryResolver::resolve`) is the second layer of
        // defense and will reject if the eventual IP is private.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_private_ranges_are_rejected() {
        // Loopback
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        // RFC1918
        assert!(is_private_ip(&"10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"172.16.0.1".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
        // Link-local
        assert!(is_private_ip(&"169.254.169.254".parse().unwrap()));
        assert!(is_private_ip(&"169.254.0.1".parse().unwrap()));
        // Unspecified
        assert!(is_private_ip(&"0.0.0.0".parse().unwrap()));
        // Broadcast
        assert!(is_private_ip(&"255.255.255.255".parse().unwrap()));
        // Documentation 203.0.113.0/24
        assert!(is_private_ip(&"203.0.113.1".parse().unwrap()));
        // CGNAT 100.64.0.0/10
        assert!(is_private_ip(&"100.64.0.1".parse().unwrap()));
        assert!(is_private_ip(&"100.127.255.254".parse().unwrap()));
        // 0.0.0.0/8 "this network"
        assert!(is_private_ip(&"0.1.2.3".parse().unwrap()));
    }

    #[test]
    fn ipv4_public_addresses_are_allowed() {
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip(&"1.1.1.1".parse().unwrap()));
        assert!(!is_private_ip(&"93.184.216.34".parse().unwrap()));
        // 100.128.0.1 is just outside CGNAT range
        assert!(!is_private_ip(&"100.128.0.1".parse().unwrap()));
    }

    #[test]
    fn ipv6_private_ranges_are_rejected() {
        // Loopback
        assert!(is_private_ip(&"::1".parse().unwrap()));
        // Unspecified
        assert!(is_private_ip(&"::".parse().unwrap()));
        // Multicast
        assert!(is_private_ip(&"ff02::1".parse().unwrap()));
        // Unique-local fc00::/7
        assert!(is_private_ip(&"fc00::1".parse().unwrap()));
        assert!(is_private_ip(&"fd00::1".parse().unwrap()));
        // Link-local fe80::/10
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
    }

    #[test]
    fn ipv4_mapped_ipv6_is_reduced_and_rejected() {
        // ::ffff:127.0.0.1 — IPv4-mapped IPv6, must be caught via IPv4 reduction
        assert!(is_private_ip(&"::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:169.254.169.254".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"::ffff:192.168.0.1".parse().unwrap()));
        // Public IPv4 mapped should be allowed
        assert!(!is_private_ip(&"::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn ipv6_public_addresses_are_allowed() {
        // Google Public DNS
        assert!(!is_private_ip(&"2001:4860:4860::8888".parse().unwrap()));
        // Cloudflare
        assert!(!is_private_ip(&"2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn is_private_or_reserved_url_handles_literal_ips() {
        assert!(is_private_or_reserved_url(&Url::parse("http://127.0.0.1/").unwrap()));
        assert!(is_private_or_reserved_url(&Url::parse("http://10.0.0.1/").unwrap()));
        assert!(is_private_or_reserved_url(&Url::parse("http://169.254.169.254/").unwrap()));
        assert!(is_private_or_reserved_url(&Url::parse("http://[::1]/").unwrap()));
        assert!(is_private_or_reserved_url(&Url::parse("http://[fe80::1]/").unwrap()));
        assert!(is_private_or_reserved_url(&Url::parse("http://localhost/").unwrap()));
        // Missing host
        assert!(is_private_or_reserved_url(&Url::parse("file:///tmp/x").unwrap()));
    }

    #[test]
    fn is_private_or_reserved_url_allows_public_and_hostnames() {
        assert!(!is_private_or_reserved_url(&Url::parse("http://8.8.8.8/").unwrap()));
        assert!(!is_private_or_reserved_url(&Url::parse("https://example.com/").unwrap()));
        // Hostnames are not IP-literal; DNS check is a separate async step
        assert!(!is_private_or_reserved_url(&Url::parse("http://evil.example/").unwrap()));
    }

    #[tokio::test]
    async fn is_hostname_private_via_dns_catches_localhost() {
        // localhost resolves to 127.0.0.1 on every reasonable system
        assert!(is_hostname_private_via_dns("localhost").await);
    }

    #[tokio::test]
    async fn is_hostname_private_via_dns_allows_public_dns_provider() {
        // dns.google resolves to 8.8.8.8 / 8.8.4.4 (public)
        assert!(!is_hostname_private_via_dns("dns.google").await);
    }

    #[tokio::test]
    async fn is_hostname_private_via_dns_returns_false_on_resolution_failure() {
        // .invalid TLD is reserved by RFC 2606 and will not resolve
        assert!(!is_hostname_private_via_dns("nonexistent-host-xyz.invalid").await);
    }
}
