use tauri_app_lib::commands::browser::{is_private_ip, is_private_or_reserved_url};
use reqwest::Url;

fn url(s: &str) -> Url {
    Url::parse(s).unwrap()
}

#[test]
fn test_handoff_rejects_loopback_ipv4() {
    assert!(is_private_or_reserved_url(&url("http://127.0.0.1:8080/")));
    assert!(is_private_or_reserved_url(&url("http://127.0.0.1/")));
}

#[test]
fn test_handoff_rejects_private_ipv4() {
    assert!(is_private_or_reserved_url(&url("http://192.168.1.1/")));
    assert!(is_private_or_reserved_url(&url("http://10.0.0.1/")));
    assert!(is_private_or_reserved_url(&url("http://172.16.0.1/")));
    assert!(is_private_or_reserved_url(&url("http://172.31.255.255/")));
}

#[test]
fn test_handoff_rejects_link_local() {
    assert!(is_private_or_reserved_url(&url(
        "http://169.254.169.254/latest/meta-data/"
    )));
}

#[test]
fn test_handoff_rejects_localhost() {
    assert!(is_private_or_reserved_url(&url("http://localhost/")));
    assert!(is_private_or_reserved_url(&url("http://localhost:3000/")));
}

#[test]
fn test_handoff_rejects_ipv6_loopback() {
    assert!(is_private_or_reserved_url(&url("http://[::1]/")));
}

#[test]
fn test_handoff_allows_public_ip() {
    assert!(!is_private_or_reserved_url(&url("http://93.184.216.34/")));
    assert!(!is_private_or_reserved_url(&url("http://8.8.8.8/")));
}

#[test]
fn test_handoff_allows_public_hostname() {
    assert!(!is_private_or_reserved_url(&url("http://example.com/")));
    assert!(!is_private_or_reserved_url(&url("https://github.com/")));
}

#[test]
fn test_is_private_ip_direct() {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
    assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))));
    assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))));
    assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
    assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))));
    assert!(is_private_ip(&IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))));
    assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
    assert!(!is_private_ip(&IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))));
    assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)));
    assert!(is_private_ip(&IpAddr::V6(Ipv6Addr::UNSPECIFIED)));
}
