//! D-1: SSRF defense integration tests.
//!
//! `src/download/ssrf.rs` has unit tests for the IP classification primitives.
//! These integration tests exercise the SSRF guard at a higher level —
//! verifying the defense-in-depth layers (literal-IP check → pre-flight DNS
//! check → connection-time IP classification) work together to block known
//! SSRF attack vectors:
//!
//! 1. DNS rebinding (hostname resolves to a private IP at lookup time)
//! 2. Redirect-to-intranet (public URL 302s to an internal address)
//! 3. IPv4-mapped IPv6 bypass (::ffff:127.0.0.1)
//! 4. CGNAT range (100.64.0.0/10)

use reqwest::Url;
use tauri_app_lib::download::ssrf::{
    is_hostname_private_via_dns, is_private_ip, is_private_or_reserved_url,
};

/// D-1: DNS rebinding — a hostname that resolves to 127.0.0.1 must be
/// rejected by the pre-flight DNS check. `localhost` is the only hostname
/// guaranteed to resolve to a loopback address on every platform.
#[tokio::test]
async fn ssrf_dns_rebinding_localhost_is_blocked() {
    assert!(
        is_hostname_private_via_dns("localhost").await,
        "localhost must be flagged as private via DNS resolution"
    );
}

/// D-1: A public DNS name must pass the pre-flight DNS check.
#[tokio::test]
async fn ssrf_public_hostname_passes_dns_check() {
    assert!(
        !is_hostname_private_via_dns("dns.google").await,
        "dns.google resolves to public IPs and must not be flagged"
    );
}

/// D-1: Redirect-to-intranet — the literal-IP URL check must catch a URL
/// that directly uses a private IP, even if the original request was to a
/// public hostname. This simulates what happens when reqwest follows a 302
/// redirect to an internal address: the engine's connection-time guard
/// re-checks the target IP.
#[test]
fn ssrf_literal_ip_check_catches_intranet_redirect_targets() {
    // Simulate redirect targets that an attacker might 302 to.
    let intranet_urls = [
        "http://127.0.0.1/admin",
        "http://10.0.0.1/",
        "http://192.168.1.1/",
        "http://172.16.0.1/",
        "http://169.254.169.254/latest/meta-data/", // AWS metadata
        "http://[::1]/",
        "http://[fe80::1]/",
    ];
    for url_str in &intranet_urls {
        let url = Url::parse(url_str).expect("parse url");
        assert!(
            is_private_or_reserved_url(&url),
            "SSRF guard must block intranet redirect target: {url_str}"
        );
    }
}

/// D-1: IPv4-mapped IPv6 bypass — an attacker might use `::ffff:127.0.0.1`
/// to bypass a naive IPv4-only check. The SSRF guard must reduce the
/// mapped address to its IPv4 form and reject it.
#[test]
fn ssrf_ipv4_mapped_ipv6_is_caught() {
    let mapped_loopback: std::net::IpAddr = "::ffff:127.0.0.1".parse().unwrap();
    assert!(
        is_private_ip(&mapped_loopback),
        "::ffff:127.0.0.1 must be caught via IPv4 reduction"
    );

    let mapped_metadata: std::net::IpAddr = "::ffff:169.254.169.254".parse().unwrap();
    assert!(
        is_private_ip(&mapped_metadata),
        "::ffff:169.254.169.254 (AWS metadata via IPv6) must be caught"
    );

    let mapped_private: std::net::IpAddr = "::ffff:10.0.0.1".parse().unwrap();
    assert!(
        is_private_ip(&mapped_private),
        "::ffff:10.0.0.1 must be caught via IPv4 reduction"
    );
}

/// D-1: CGNAT (100.64.0.0/10) addresses must be rejected — they are
/// carrier-grade NAT and not routable on the public internet.
#[test]
fn ssrf_cgnat_range_is_rejected() {
    let start: std::net::IpAddr = "100.64.0.1".parse().unwrap();
    assert!(
        is_private_ip(&start),
        "100.64.0.1 (CGNAT start) must be rejected"
    );

    let end: std::net::IpAddr = "100.127.255.254".parse().unwrap();
    assert!(
        is_private_ip(&end),
        "100.127.255.254 (CGNAT end) must be rejected"
    );

    // Just outside CGNAT must be allowed.
    let outside: std::net::IpAddr = "100.128.0.1".parse().unwrap();
    assert!(
        !is_private_ip(&outside),
        "100.128.0.1 (outside CGNAT) must be allowed"
    );
}

/// D-1: Defense-in-depth — a URL with a literal private IP must be caught
/// by the synchronous `is_private_or_reserved_url` check, AND the
/// underlying IP must also be flagged by `is_private_ip`. This verifies
/// the two layers agree.
#[test]
fn ssrf_literal_check_and_ip_check_agree_on_private_addresses() {
    let test_cases = [
        ("http://127.0.0.1/", "127.0.0.1"),
        ("http://10.0.0.1/", "10.0.0.1"),
        ("http://192.168.1.1/", "192.168.1.1"),
        ("http://169.254.169.254/", "169.254.169.254"),
        ("http://0.0.0.0/", "0.0.0.0"),
    ];
    for (url_str, ip_str) in &test_cases {
        let url = Url::parse(url_str).expect("parse url");
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        assert!(
            is_private_or_reserved_url(&url),
            "URL check must flag {url_str}"
        );
        assert!(is_private_ip(&ip), "IP check must flag {ip_str}");
    }
}

/// D-1: Public addresses must pass both layers — no false positives on
/// legitimate public IPs.
#[test]
fn ssrf_public_addresses_pass_both_layers() {
    let public_cases = [
        ("http://8.8.8.8/", "8.8.8.8"),
        ("http://1.1.1.1/", "1.1.1.1"),
        ("http://93.184.216.34/", "93.184.216.34"),
    ];
    for (url_str, ip_str) in &public_cases {
        let url = Url::parse(url_str).expect("parse url");
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        assert!(
            !is_private_or_reserved_url(&url),
            "URL check must NOT flag public {url_str}"
        );
        assert!(
            !is_private_ip(&ip),
            "IP check must NOT flag public {ip_str}"
        );
    }
}
