//! R-3: EngineRegistry self-describing URL routing integration tests.
//!
//! `EngineRegistry::engine_for_uri` dispatches a URL to the correct engine by:
//! 1. Sorting engines by `priority()` descending (stable sort preserves
//!    registration order for same-priority engines).
//! 2. Iterating and returning the first engine whose `matches_url()` returns
//!    true (content-level matching: magnet scheme, `.torrent`/`.m3u8`/`.mpd`/
//!    `.meta4`/`.metalink` suffixes).
//! 3. Falling back to the first engine whose `supports_scheme()` accepts the
//!    URL scheme (HTTP/FTP/SFTP/WebDAV).
//!
//! These tests verify each engine's `matches_url`/`priority`/`supports_scheme`
//! trait methods directly on concrete engine instances, then verify the
//! dispatch order by replicating `engine_for_uri`'s priority-sorted iteration.
//!
//! Note: `BtEngine` is NOT imported here because its struct definition
//! transitively depends on `librqbit` native DLLs that can trigger
//! `STATUS_ENTRYPOINT_NOT_FOUND` on some Windows environments. BT routing
//! predicates are tested via `is_torrent_url` + magnet scheme check instead.
//! The dispatch test uses a `Spec` struct (not `&dyn DownloadEngine`) so a
//! mock BT entry can be included without linking `librqbit`.

use std::sync::Arc;

use tauri_app_lib::download::{
    DashEngine, DownloadEngine, FtpEngine, HlsEngine, HttpEngine, MetalinkEngine, SftpEngine,
    WebDavEngine, url_classify::is_torrent_url,
};
use tauri_app_lib::proxy::ResolvedProxyConfig;

fn proxy_config() -> tauri_app_lib::proxy::SharedProxyConfig {
    ResolvedProxyConfig::shared_default()
}

fn http_engine() -> Arc<HttpEngine> {
    Arc::new(
        HttpEngine::with_proxy_config(ResolvedProxyConfig::shared_default())
            .expect("HTTP engine init"),
    )
}

fn parse(url: &str) -> reqwest::Url {
    reqwest::Url::parse(url).expect("valid URL")
}

// --- BT routing predicates (tested without BtEngine to avoid librqbit DLL) --
//
// `BtEngine::matches_url` = `url.scheme() == "magnet" || is_torrent_url(url)`
// `BtEngine::priority` = 100
// `BtEngine::supports_scheme` = `matches!(scheme, "magnet" | "file")`

/// Replicates `BtEngine::matches_url` without constructing `BtEngine`.
fn bt_matches_url(url: &reqwest::Url) -> bool {
    url.scheme() == "magnet" || is_torrent_url(url)
}

#[test]
fn bt_predicate_routes_magnet_url() {
    assert!(
        bt_matches_url(&parse("magnet:?xt=urn:btih:abcdef0123456789")),
        "magnet URLs must match BT predicate"
    );
}

#[test]
fn bt_predicate_routes_torrent_file_url() {
    assert!(
        bt_matches_url(&parse("https://example.com/linux-distro.torrent")),
        ".torrent suffix must match BT predicate"
    );
    assert!(
        bt_matches_url(&parse("file:///C:/downloads/file.torrent")),
        "file:// .torrent must match BT predicate"
    );
}

#[test]
fn bt_predicate_does_not_match_non_torrent_urls() {
    assert!(
        !bt_matches_url(&parse("https://example.com/file.zip")),
        "plain HTTPS must not match BT predicate"
    );
    assert!(
        !bt_matches_url(&parse("https://example.com/playlist.m3u8")),
        ".m3u8 must not match BT predicate"
    );
}

// --- Metalink engine routing ----------------------------------------------

#[test]
fn metalink_engine_routes_meta4_and_metalink_urls() {
    let engine = MetalinkEngine::new(http_engine());
    assert_eq!(engine.id(), "metalink");
    assert_eq!(engine.priority(), 90);
    assert!(
        engine.matches_url(&parse("https://example.com/manifest.meta4")),
        ".meta4 must match MetalinkEngine"
    );
    assert!(
        engine.matches_url(&parse("https://example.com/manifest.metalink")),
        ".metalink must match MetalinkEngine"
    );
    assert!(
        !engine.matches_url(&parse("https://example.com/file.zip")),
        "plain HTTPS must not match MetalinkEngine"
    );
}

// --- HLS engine routing ----------------------------------------------------

#[test]
fn hls_engine_routes_m3u8_urls() {
    let engine = HlsEngine::new(http_engine());
    assert_eq!(engine.id(), "hls");
    assert_eq!(engine.priority(), 80);
    assert!(
        engine.matches_url(&parse("https://example.com/playlist.m3u8")),
        ".m3u8 must match HlsEngine"
    );
    assert!(
        !engine.matches_url(&parse("https://example.com/file.zip")),
        "plain HTTPS must not match HlsEngine"
    );
}

// --- DASH engine routing ---------------------------------------------------

#[test]
fn dash_engine_routes_mpd_urls() {
    let engine = DashEngine::new(http_engine());
    assert_eq!(engine.id(), "dash");
    assert_eq!(engine.priority(), 70);
    assert!(
        engine.matches_url(&parse("https://example.com/stream.mpd")),
        ".mpd must match DashEngine"
    );
    assert!(
        !engine.matches_url(&parse("https://example.com/file.zip")),
        "plain HTTPS must not match DashEngine"
    );
}

// --- HTTP engine (scheme fallback) -----------------------------------------

#[test]
fn http_engine_supports_http_and_https() {
    let engine = http_engine();
    assert_eq!(engine.id(), "http");
    assert_eq!(engine.priority(), 0);
    assert!(
        !engine.matches_url(&parse("https://example.com/file.zip")),
        "HttpEngine.matches_url must return false (scheme fallback only)"
    );
    assert!(engine.supports_scheme("http"));
    assert!(engine.supports_scheme("https"));
}

// --- FTP / SFTP / WebDAV engines (scheme fallback) -------------------------

#[test]
fn ftp_engine_routes_by_scheme() {
    let engine = FtpEngine::new(proxy_config());
    assert_eq!(engine.id(), "ftp");
    assert_eq!(engine.priority(), 0);
    assert!(!engine.matches_url(&parse("ftp://example.com/file")));
    assert!(engine.supports_scheme("ftp"));
    assert!(!engine.supports_scheme("https"));
}

#[test]
fn sftp_engine_routes_by_scheme() {
    let engine = SftpEngine::new(proxy_config());
    assert_eq!(engine.id(), "sftp");
    assert_eq!(engine.priority(), 0);
    assert!(!engine.matches_url(&parse("sftp://example.com/file")));
    assert!(engine.supports_scheme("sftp"));
}

#[test]
fn webdav_engine_routes_by_scheme() {
    let engine = WebDavEngine::new(http_engine());
    assert_eq!(engine.id(), "webdav");
    assert_eq!(engine.priority(), 0);
    assert!(engine.supports_scheme("webdav"));
    assert!(engine.supports_scheme("webdavs"));
}

// --- Priority dispatch order ----------------------------------------------
//
// Replicates `engine_for_uri`'s dispatch: sort by priority descending,
// iterate `matches_url`, fall back to `supports_scheme`. Uses a `Spec` struct
// (not `&dyn DownloadEngine`) so a mock BT entry can be included without
// linking `librqbit` (whose native DLLs cause `STATUS_ENTRYPOINT_NOT_FOUND`
// on some Windows environments).

#[derive(Clone)]
struct Spec {
    id: &'static str,
    priority: i32,
    matches_url: fn(&reqwest::Url) -> bool,
    supports_scheme: fn(&str) -> bool,
}

fn route_url(engines: &[Spec], url: &str) -> Result<String, String> {
    let parsed = parse(url);
    let scheme = parsed.scheme();
    let mut sorted = engines.to_vec();
    sorted.sort_by_key(|e| std::cmp::Reverse(e.priority));
    for engine in &sorted {
        if (engine.matches_url)(&parsed) {
            return Ok(engine.id.to_string());
        }
    }
    for engine in &sorted {
        if (engine.supports_scheme)(scheme) {
            return Ok(engine.id.to_string());
        }
    }
    Err(format!("The {scheme} protocol is not supported yet."))
}

// Trait-method adapters: wrap `&self` methods into `fn` pointers for `Spec`.
fn metalink_matches(url: &reqwest::Url) -> bool {
    MetalinkEngine::new(http_engine()).matches_url(url)
}
fn metalink_scheme(s: &str) -> bool {
    MetalinkEngine::new(http_engine()).supports_scheme(s)
}
fn hls_matches(url: &reqwest::Url) -> bool {
    HlsEngine::new(http_engine()).matches_url(url)
}
fn hls_scheme(s: &str) -> bool {
    HlsEngine::new(http_engine()).supports_scheme(s)
}
fn dash_matches(url: &reqwest::Url) -> bool {
    DashEngine::new(http_engine()).matches_url(url)
}
fn dash_scheme(s: &str) -> bool {
    DashEngine::new(http_engine()).supports_scheme(s)
}
fn http_scheme(s: &str) -> bool {
    HttpEngine::with_proxy_config(ResolvedProxyConfig::shared_default())
        .expect("HTTP engine init")
        .supports_scheme(s)
}
fn http_matches(_url: &reqwest::Url) -> bool {
    false // HttpEngine::matches_url always returns false (scheme fallback)
}
fn ftp_matches(_url: &reqwest::Url) -> bool {
    false
}
fn ftp_scheme(s: &str) -> bool {
    FtpEngine::new(proxy_config()).supports_scheme(s)
}
fn sftp_matches(_url: &reqwest::Url) -> bool {
    false
}
fn sftp_scheme(s: &str) -> bool {
    SftpEngine::new(proxy_config()).supports_scheme(s)
}
fn webdav_matches(_url: &reqwest::Url) -> bool {
    false
}
fn webdav_scheme(s: &str) -> bool {
    WebDavEngine::new(http_engine()).supports_scheme(s)
}
fn bt_scheme(s: &str) -> bool {
    matches!(s, "magnet" | "file")
}

#[test]
fn dispatch_priority_prefers_hls_over_http_for_m3u8() {
    let engines: Vec<Spec> = vec![
        Spec { id: "bt", priority: 100, matches_url: bt_matches_url, supports_scheme: bt_scheme },
        Spec { id: "metalink", priority: 90, matches_url: metalink_matches, supports_scheme: metalink_scheme },
        Spec { id: "hls", priority: 80, matches_url: hls_matches, supports_scheme: hls_scheme },
        Spec { id: "dash", priority: 70, matches_url: dash_matches, supports_scheme: dash_scheme },
        Spec { id: "webdav", priority: 0, matches_url: webdav_matches, supports_scheme: webdav_scheme },
        Spec { id: "http", priority: 0, matches_url: http_matches, supports_scheme: http_scheme },
        Spec { id: "ftp", priority: 0, matches_url: ftp_matches, supports_scheme: ftp_scheme },
        Spec { id: "sftp", priority: 0, matches_url: sftp_matches, supports_scheme: sftp_scheme },
    ];

    // `.m3u8` on https must route to HLS (priority 80), not HTTP (priority 0).
    assert_eq!(
        route_url(&engines, "https://example.com/playlist.m3u8").unwrap(),
        "hls"
    );
    // `.torrent` on https must route to BT (priority 100).
    assert_eq!(
        route_url(&engines, "https://example.com/file.torrent").unwrap(),
        "bt"
    );
    // `.mpd` on https must route to DASH (priority 70).
    assert_eq!(
        route_url(&engines, "https://example.com/stream.mpd").unwrap(),
        "dash"
    );
    // `.meta4` on https must route to Metalink (priority 90).
    assert_eq!(
        route_url(&engines, "https://example.com/manifest.meta4").unwrap(),
        "metalink"
    );
    // Plain https must fall through to HTTP.
    assert_eq!(
        route_url(&engines, "https://example.com/file.zip").unwrap(),
        "http"
    );
    // magnet must route to BT.
    assert_eq!(
        route_url(&engines, "magnet:?xt=urn:btih:abc").unwrap(),
        "bt"
    );
    // ftp must route to FTP.
    assert_eq!(
        route_url(&engines, "ftp://example.com/file").unwrap(),
        "ftp"
    );
    // sftp must route to SFTP.
    assert_eq!(
        route_url(&engines, "sftp://example.com/file").unwrap(),
        "sftp"
    );
    // webdav must route to WebDAV.
    assert_eq!(
        route_url(&engines, "webdav://example.com/file").unwrap(),
        "webdav"
    );
    // Unsupported scheme must error.
    assert!(route_url(&engines, "custom://example.com/file").is_err());
}
