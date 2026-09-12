//! SEC-03: the single authority for building HTTP clients in the backend.
//!
//! Every outgoing HTTP request must use a client produced here — per-engine or
//! ad-hoc `Client::builder()` calls silently drop the shared policy stack:
//! proxy resolution (Off must mean `no_proxy()`, never "fall through to the
//! system proxy"), the SSRF connection-time resolver filter, the SSRF redirect
//! policy, and connection pooling. `tests/source_hygiene.rs` enforces that
//! `Client::builder(` appears nowhere else.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use hickory_resolver::{config::*, TokioResolver};
use reqwest::{Client, NoProxy, Proxy};
use tokio::sync::RwLock;

use crate::proxy::{AppProxyMode, ResolvedProxyConfig};

/// Shared client cache keyed by proxy fingerprint.
///
/// Held as `Arc` by the engine registry so HTTP and its derived engines (E-4)
/// plus the previously ad-hoc builders (BT torrent fetch, checksum sidecars)
/// share one cache and one invalidation path.
#[derive(Debug, Clone, Default)]
pub struct NetworkClientFactory {
    clients: Arc<RwLock<HashMap<String, Client>>>,
}

impl NetworkClientFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a cached client for an explicit resolved proxy configuration.
    ///
    /// FUN-02: task overrides (Inherit/Off/Custom) must go through here with
    /// the task-resolved config instead of the global client.
    pub async fn client_for(&self, config: &ResolvedProxyConfig) -> Result<Client, String> {
        let fingerprint = config.fingerprint();
        {
            let cache = self.clients.read().await;
            if let Some(client) = cache.get(&fingerprint) {
                return Ok(client.clone());
            }
        }
        let client = build_client(config)?;
        self.clients
            .write()
            .await
            .insert(fingerprint, client.clone());
        Ok(client)
    }

    /// Clear the cache when proxy configuration changes so subsequent
    /// requests build fresh clients with the new settings.
    pub async fn invalidate(&self) {
        self.clients.write().await.clear();
    }

    /// Current cache entry count (test/observability aid).
    pub async fn cache_len(&self) -> usize {
        self.clients.read().await.len()
    }
}

/// DNS resolver adapter wrapping hickory-resolver to implement reqwest's `Resolve` trait.
/// hickory-resolver provides built-in caching, avoiding repeated system DNS lookups
/// when multiple downloads target the same host.
#[derive(Clone, Debug)]
struct HickoryResolver(TokioResolver);

impl HickoryResolver {
    /// A-5: Returns `Result` instead of `.expect()` — system DNS configuration
    /// can be missing/corrupted (containers, chroot, minimal images), and a
    /// panic here would crash the worker or the synchronous `set_proxy_config`
    /// path. The caller (`build_client`) propagates the error as a string.
    ///
    /// Uses `TokioResolver::builder_tokio()` to read the OS DNS configuration
    /// (Windows registry / `/etc/resolv.conf`). In hickory-resolver 0.26,
    /// `ResolverConfig::default()` produces an **empty** config with zero name
    /// servers, so every DNS lookup fails with "error sending request for url".
    /// Reading the system config populates real upstream DNS servers.
    fn new() -> Result<Self, String> {
        let resolver = match TokioResolver::builder_tokio() {
            Ok(builder) => builder.build(),
            Err(_) => {
                // Fallback for environments where system DNS config is
                // unavailable (containers, chroot, minimal images).
                TokioResolver::builder_with_config(
                    ResolverConfig::default(),
                    hickory_resolver::net::runtime::TokioRuntimeProvider::new(),
                )
                .with_options(ResolverOpts::default())
                .build()
            }
        }
        .map_err(|e| format!("DNS resolver unavailable: {e}"))?;
        Ok(Self(resolver))
    }
}

impl reqwest::dns::Resolve for HickoryResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = self.0.clone();
        Box::pin(async move {
            let lookup = resolver.lookup_ip(name.as_str()).await.map_err(|e| {
                Box::new(std::io::Error::other(e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;
            // A-2: SSRF connection-time guard — filter out any resolved IP
            // that is private or reserved. If ALL resolved IPs are filtered,
            // return an error so the connection fails rather than falling
            // back to a private address. This is the second layer of defense
            // after the handoff pre-flight DNS check; it also protects
            // non-handoff paths (direct UI/clipboard task creation) where
            // the pre-flight check does not run.
            let addrs: Vec<SocketAddr> = lookup
                .iter()
                .filter(|ip| !crate::download::ssrf::is_private_ip(ip))
                .map(|ip| SocketAddr::new(ip, 0))
                .collect();
            // Return an error (not an empty iterator) when all IPs are filtered. reqwest
            // treats an empty address list as a DNS failure and may retry/hang; an explicit
            // error fails the connection fast with a diagnosable message.
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::other(
                    "SSRF guard: all resolved IPs are private or reserved",
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A-2: Custom reqwest redirect policy that re-checks each redirect hop's
/// target URL against the SSRF guard. Without this, a public URL could 302
/// to an internal address (`http://127.0.0.1/...`, `http://169.254.169.254/...`)
/// and reqwest would follow it without question.
///
/// Combined with the connection-time resolver filter (`HickoryResolver::resolve`),
/// this provides defense in depth: even if a redirect slips through to a
/// hostname that resolves to a private IP, the resolver will refuse to
/// connect.
fn ssrf_safe_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        // Limit to 10 hops (matches the prior `Policy::limited(10)` behavior).
        // `previous()` includes the initial URL as the first entry, so
        // `len() > 10` means 10 redirects have already been followed.
        if attempt.previous().len() > 10 {
            return attempt.error(std::io::Error::other("too many redirects"));
        }
        if crate::download::ssrf::is_private_or_reserved_url(attempt.url()) {
            // `stop` returns the 3xx response to the caller rather than
            // following the redirect to a private address. The caller will
            // see the redirect status and handle it as a non-2xx response.
            // The connection-time resolver guard is the authoritative layer;
            // stopping the redirect is sufficient to prevent the request from
            // reaching the internal target.
            return attempt.stop();
        }
        attempt.follow()
    })
}

pub fn build_client(config: &ResolvedProxyConfig) -> Result<Client, String> {
    let resolver = HickoryResolver::new()?;
    let mut builder = Client::builder()
        .redirect(ssrf_safe_redirect_policy())
        .user_agent(concat!("VibeDownloader/", env!("CARGO_PKG_VERSION")))
        .dns_resolver(Arc::new(resolver))
        // Only set the connection establishment timeout; streaming downloads should not be subject
        // to an overall timeout, otherwise large file downloads would be interrupted after 60s.
        // Application-layer timeouts are set per-engine on the Response as needed.
        .connect_timeout(Duration::from_secs(30))
        // Connection pool optimization (E-10): increase per-host idle connections, set 90s idle timeout,
        // to avoid repeated handshakes when downloading from the same host at short intervals.
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90));

    match config.mode {
        AppProxyMode::Off => {
            builder = builder.no_proxy();
        }
        AppProxyMode::System => {}
        AppProxyMode::Custom => {
            let url = config
                .url
                .as_deref()
                .ok_or_else(|| "Custom proxy URL is not configured.".to_string())?;
            let mut proxy =
                Proxy::all(url).map_err(|_| "Custom proxy URL is invalid.".to_string())?;
            if let Some(no_proxy) = config.no_proxy.as_deref() {
                proxy = proxy.no_proxy(NoProxy::from_string(no_proxy));
            }
            if let Some(username) = &config.username {
                proxy = proxy.basic_auth(username, config.password.as_deref().unwrap_or(""));
            }
            builder = builder.proxy(proxy);
        }
    }

    builder
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", sanitize_proxy_error(e)))
}

fn sanitize_proxy_error(error: reqwest::Error) -> String {
    let message = error.to_string();
    if message.contains('@') {
        "proxy configuration is invalid".to_string()
    } else {
        message
    }
}
