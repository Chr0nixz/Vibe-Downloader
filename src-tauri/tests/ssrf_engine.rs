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

// ---------------------------------------------------------------------------
// SEC-10 / SEC-12: connect-time authority checks
// ---------------------------------------------------------------------------

/// Waits briefly for an in-flight connection attempt, then asserts the
/// listener accepted nothing (the guard must reject before any socket I/O).
fn assert_no_connection(listener: &std::net::TcpListener) {
    listener
        .set_nonblocking(true)
        .expect("set listener nonblocking");
    std::thread::sleep(std::time::Duration::from_millis(200));
    match listener.accept() {
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
        Ok(_) => panic!("guard must reject the target before any connection is made"),
        Err(error) => panic!("unexpected listener error: {error}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sec10_literal_private_ip_is_rejected_before_any_connection() {
    // SEC-10: IP literals never reach the client's dns_resolver, so the
    // request-site authority pre-flight is the only layer that sees them.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");

    let engine = tauri_app_lib::download::HttpEngine::new().expect("engine");
    let error = engine
        .probe_with_headers_and_proxy(&format!("http://{addr}/file"), &[], None)
        .await
        .expect_err("literal private target must be rejected");
    assert!(
        error.contains("SSRF guard"),
        "expected SSRF rejection, got {error}"
    );

    assert_no_connection(&listener);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sec12_ftp_probe_rejects_hostname_resolving_to_private_ip() {
    // SEC-12: FTP control connections bypass reqwest entirely; the DNS
    // pre-flight must block localhost before the engine dials the port.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();

    use tauri_app_lib::download::DownloadEngine as _;
    let engine = tauri_app_lib::download::FtpEngine::new(
        tauri_app_lib::proxy::ResolvedProxyConfig::shared_default(),
    );
    let error = engine
        .probe(tauri_app_lib::download::ProbeRequest {
            uri: format!("ftp://localhost:{port}/file.bin"),
            ..tests_new_ftp_probe_request()
        })
        .await
        .expect_err("intranet FTP target must be rejected");
    let error_text = error.to_string();
    assert!(
        error_text.contains("intranet_target_blocked"),
        "expected intranet_target_blocked, got {error_text}"
    );

    assert_no_connection(&listener);
}

fn tests_new_ftp_probe_request() -> tauri_app_lib::download::ProbeRequest {
    // Mirrors ftp_engine.rs::new_probe_request; kept local so this file does
    // not depend on that test module's helpers.
    tauri_app_lib::download::ProbeRequest {
        uri: String::new(),
        source: None,
        request_headers: Vec::new(),
        pool: None,
        task_id: None,
        credentials: None,
        proxy_config: None,
        app: None,
        request_id: None,
        cancel_token: None,
        network_policy: tauri_app_lib::download::network_policy::NetworkPolicy::default(),
    }
}
