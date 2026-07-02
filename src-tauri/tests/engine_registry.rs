//! E-4: HTTP client cache sharing across HLS/DASH/Metalink/WebDAV engines.
//!
//! `EngineRegistry::new` constructs a single `Arc<HttpEngine>` and passes it to
//! the four HTTP-derived engines (Metalink/HLS/DASH/WebDAV). They share the
//! same client cache (keyed by proxy fingerprint), so proxy invalidation via
//! `set_proxy_config` → `invalidate_clients` covers all four engines in one
//! call.
//!
//! These tests verify:
//! 1. `HttpEngine::client()` populates the cache and repeated calls reuse it.
//! 2. `invalidate_clients()` clears the cache (the path `set_proxy_config`
//!    invokes on the shared `Arc<HttpEngine>`).
//! 3. The four derived engine constructors accept the same `Arc<HttpEngine>`,
//!    verifying at compile time that they share the client cache.
//!
//! Note: We construct `HttpEngine` and the derived engines directly rather
//! than via `EngineRegistry::new()` to avoid linking `BtEngine`'s vtable
//! methods (which pull in `librqbit` native dependencies that can trigger
//! `STATUS_ENTRYPOINT_NOT_FOUND` on some Windows environments).

use std::sync::Arc;

use tauri_app_lib::download::{DashEngine, HlsEngine, HttpEngine, MetalinkEngine, WebDavEngine};
use tauri_app_lib::proxy::ResolvedProxyConfig;

fn http_engine() -> Arc<HttpEngine> {
    Arc::new(
        HttpEngine::with_proxy_config(ResolvedProxyConfig::shared_default())
            .expect("HTTP engine init"),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_cache_starts_empty_and_populates_on_first_call() {
    let engine = http_engine();
    assert_eq!(engine.client_cache_len().await, 0, "cache must start empty");

    let _client = engine
        .client()
        .await
        .expect("client build should succeed with default proxy config");

    let cached = engine.client_cache_len().await;
    assert!(
        cached >= 1,
        "client() must populate the cache, got {cached} entries"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_client_calls_reuse_cached_entry() {
    let engine = http_engine();

    let _first = engine.client().await.expect("first client build");
    let after_first = engine.client_cache_len().await;

    let _second = engine.client().await.expect("second client build");
    let after_second = engine.client_cache_len().await;

    assert_eq!(
        after_first, after_second,
        "repeated client() with same proxy must not grow the cache"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalidate_clients_clears_shared_cache() {
    // E-4: set_proxy_config calls http_engine.invalidate_clients(), which
    // clears the shared cache. Since all 4 derived engines hold the same
    // Arc<HttpEngine>, this single invalidation covers them all — no engine
    // retains a stale client built with the old proxy.
    let engine = http_engine();

    // Populate the cache.
    let _client = engine.client().await.expect("client build");
    assert!(
        engine.client_cache_len().await >= 1,
        "cache should have at least one entry before invalidation"
    );

    // Trigger invalidation.
    engine.invalidate_clients().await;

    assert_eq!(
        engine.client_cache_len().await,
        0,
        "invalidate_clients must clear the cache so all 4 derived engines rebuild clients"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_rebuilds_after_invalidation() {
    // After invalidation, the next client() call must rebuild and re-cache.
    let engine = http_engine();

    let _first = engine.client().await.expect("first client");
    let count_after_first = engine.client_cache_len().await;

    engine.invalidate_clients().await;
    assert_eq!(engine.client_cache_len().await, 0);

    let _second = engine.client().await.expect("second client after invalidation");
    let count_after_second = engine.client_cache_len().await;

    assert_eq!(
        count_after_first, count_after_second,
        "cache must be repopulated after invalidation with same count"
    );
}

#[test]
fn four_derived_engines_accept_shared_http_engine() {
    // E-4 compile-time verification: Metalink/HLS/DASH/WebDAV constructors
    // all accept `Arc<HttpEngine>`, proving they share the same client cache.
    // If any constructor signature changes, this test fails to compile.
    let http = http_engine();
    let _metalink = MetalinkEngine::new(http.clone());
    let _hls = HlsEngine::new(http.clone());
    let _dash = DashEngine::new(http.clone());
    let _webdav = WebDavEngine::new(http.clone());

    // The shared Arc is still alive (4 engines + this reference).
    assert_eq!(
        Arc::strong_count(&http),
        5,
        "4 derived engines + local binding must share the same Arc<HttpEngine>"
    );
}
