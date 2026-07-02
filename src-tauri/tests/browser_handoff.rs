use tauri_app_lib::commands::browser::{
    is_private_ip, is_private_or_reserved_url, validate_handoff_file_path,
    FORWARDED_HEADER_ALLOWLIST,
};
use reqwest::Url;
use serial_test::serial;
use std::fs;
use std::path::PathBuf;

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

// --- Header allowlist consistency (A-4) ------------------------------------
//
// The browser extension's `background.js` keeps its own `ALLOWED_HEADER_NAMES`
// set that filters which request headers are forwarded to the desktop app
// over Native Messaging. The Rust side (`FORWARDED_HEADER_ALLOWLIST`) then
// re-validates the same set at the handoff boundary. Drift between the two
// would either silently drop headers (JS narrower than Rust) or surface
// `forbidden_header` rejections (Rust narrower than JS).
//
// The authoritative drift check lives in `scripts/verify-extension-manifest.mjs`
// (assertion 4) because it can read both source files directly. This Rust
// test is a defensive second line: it pins the Rust constant to a known set
// so any accidental mutation of `FORWARDED_HEADER_ALLOWLIST` is caught at
// `cargo test` time, before the JS-side check even runs.
//
// If you intentionally add/remove an entry here, also update the matching
// `ALLOWED_HEADER_NAMES` set in `browser/extension-core/src/background.js`
// and run `pnpm verify:manifest` to confirm the cross-language check still
// passes.

#[test]
fn test_forwarded_header_allowlist_matches_expected_set() {
    let mut expected: Vec<&str> = FORWARDED_HEADER_ALLOWLIST.to_vec();
    expected.sort_unstable();

    let mut reference: Vec<&str> = vec![
        "cookie",
        "user-agent",
        "referer",
        "origin",
        "accept",
        "accept-language",
        "dnt",
        "cache-control",
        "pragma",
    ];
    reference.sort_unstable();

    assert_eq!(
        expected, reference,
        "FORWARDED_HEADER_ALLOWLIST drifted from the expected set. If this is intentional, \
         update browser/extension-core/src/background.js ALLOWED_HEADER_NAMES to match, \
         then run `pnpm verify:manifest` to confirm the cross-language check passes."
    );
}

#[test]
fn test_forwarded_header_allowlist_is_lowercase() {
    // The header lookup in `create_browser_handoff_task_with_state` lowercases
    // header names before consulting the allowlist, so every entry here must
    // be lowercase or the lookup will silently miss.
    for entry in FORWARDED_HEADER_ALLOWLIST {
        assert!(
            entry.chars().all(|c| !c.is_ascii_uppercase()),
            "FORWARDED_HEADER_ALLOWLIST entry `{entry}` must be lowercase; \
             header lookups lowercase the incoming name before comparison."
        );
    }
}

#[test]
fn test_forwarded_header_allowlist_rejects_authorization() {
    // The `Authorization` header is explicitly rejected at the handoff
    // boundary so browser-managed credentials cannot leak into the desktop
    // app's task store. This is a hard security invariant — adding it to
    // the allowlist would be a regression.
    assert!(
        !FORWARDED_HEADER_ALLOWLIST
            .iter()
            .any(|name| name.eq_ignore_ascii_case("authorization")),
        "Authorization must never appear in FORWARDED_HEADER_ALLOWLIST; \
         browser handoff must not forward credentials."
    );
}

// --- S-2.1 handoff file path validation ------------------------------------
//
// `validate_handoff_file_path` enforces three invariants before the main
// process reads or deletes a `--browser-handoff-file`:
//   1. The canonicalized path must be inside `VIBE_DOWNLOADER_HANDOFF_DIR`.
//   2. The file name stem must match `safe_file_stem` rules (alphanumeric +
//      `-` + `_`, non-empty, ≤128 chars) with a `.json` extension.
//   3. The file size must not exceed 1 MiB.
//
// These tests set `VIBE_DOWNLOADER_HANDOFF_DIR` to an isolated temp subdir
// and run under `serial` to avoid env-var races with parallel tests.

fn unique_handoff_dir(label: &str) -> PathBuf {
    let id = rand::random::<u64>();
    std::env::temp_dir().join(format!("vibe-handoff-test-{label}-{id}"))
}

#[test]
#[serial]
fn test_validate_handoff_file_path_rejects_outside_dir() {
    let dir = unique_handoff_dir("outside");
    fs::create_dir_all(&dir).unwrap();
    std::env::set_var("VIBE_DOWNLOADER_HANDOFF_DIR", &dir);

    // A path outside the handoff dir — use a temp file in the parent temp dir
    // with a valid name so only the containment check fails.
    let outside = std::env::temp_dir().join(format!(
        "vibe-handoff-outside-{}.json",
        rand::random::<u64>()
    ));
    fs::write(&outside, b"{}").unwrap();

    let result = validate_handoff_file_path(&outside);
    assert!(
        result.is_err(),
        "path outside handoff_dir must be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("must be inside"),
        "error should mention containment: {err}"
    );

    fs::remove_file(&outside).ok();
    std::env::remove_var("VIBE_DOWNLOADER_HANDOFF_DIR");
    fs::remove_dir_all(&dir).ok();
}

#[test]
#[serial]
fn test_validate_handoff_file_path_rejects_oversize() {
    let dir = unique_handoff_dir("oversize");
    fs::create_dir_all(&dir).unwrap();
    std::env::set_var("VIBE_DOWNLOADER_HANDOFF_DIR", &dir);

    let path = dir.join(format!("valid-{}.json", rand::random::<u64>()));
    // 2 MiB — exceeds the 1 MiB limit.
    let oversize = vec![b' '; 2 * 1024 * 1024];
    fs::write(&path, &oversize).unwrap();

    let result = validate_handoff_file_path(&path);
    assert!(
        result.is_err(),
        "file exceeding 1 MiB must be rejected"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("exceeds max"),
        "error should mention size limit: {err}"
    );

    std::env::remove_var("VIBE_DOWNLOADER_HANDOFF_DIR");
    fs::remove_dir_all(&dir).ok();
}

#[test]
#[serial]
fn test_validate_handoff_file_path_rejects_wrong_name() {
    let dir = unique_handoff_dir("wrongname");
    fs::create_dir_all(&dir).unwrap();
    std::env::set_var("VIBE_DOWNLOADER_HANDOFF_DIR", &dir);

    // Each (file_name, should_pass) case. All files are inside handoff_dir;
    // only the name should cause rejection.
    let cases: &[(&str, bool)] = &[
        // Valid names.
        ("abc123.json", true),
        ("request-id-2026.json", true),
        ("req_2026_06_29.json", true),
        // Invalid: wrong extension.
        ("valid.txt", false),
        // Invalid: empty stem.
        (".json", false),
        // Invalid: contains dot in stem (not allowed by safe_file_stem).
        ("req.123.json", false),
    ];

    for (name, should_pass) in cases {
        let path = dir.join(name);
        fs::write(&path, b"{}").unwrap();
        let result = validate_handoff_file_path(&path);
        if *should_pass {
            assert!(
                result.is_ok(),
                "name `{name}` should be accepted, got: {:?}",
                result.err()
            );
        } else {
            assert!(
                result.is_err(),
                "name `{name}` should be rejected"
            );
        }
        fs::remove_file(&path).ok();
    }

    std::env::remove_var("VIBE_DOWNLOADER_HANDOFF_DIR");
    fs::remove_dir_all(&dir).ok();
}
