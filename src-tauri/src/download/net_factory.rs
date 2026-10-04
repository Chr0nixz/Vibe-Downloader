//! SEC-03: the single authority for building HTTP clients in the backend.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use hickory_resolver::{config::*, TokioResolver};
use reqwest::{Client, NoProxy, Proxy};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::{
    download::network_policy::NetworkPolicy,
    proxy::{AppProxyMode, ResolvedProxyConfig},
};

#[derive(Debug, Clone, Default)]
pub struct NetworkClientFactory {
    clients: Arc<RwLock<HashMap<String, Client>>>,
}

impl NetworkClientFactory {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn client_for(&self, config: &ResolvedProxyConfig) -> Result<Client, String> {
        self.client_for_policy(config, &NetworkPolicy::default())
            .await
    }

    pub async fn client_for_policy(
        &self,
        config: &ResolvedProxyConfig,
        policy: &NetworkPolicy,
    ) -> Result<Client, String> {
        let password_hash = format!(
            "{:x}",
            Sha256::digest(config.password.as_deref().unwrap_or_default().as_bytes())
        );
        let fingerprint = format!(
            "{}:{}:{}",
            config.fingerprint(),
            password_hash,
            policy.fingerprint()
        );
        if let Some(client) = self.clients.read().await.get(&fingerprint) {
            return Ok(client.clone());
        }
        let client = build_client_with_policy(config, policy.clone())?;
        self.clients
            .write()
            .await
            .insert(fingerprint, client.clone());
        Ok(client)
    }

    pub async fn invalidate(&self) {
        self.clients.write().await.clear();
    }
    pub async fn cache_len(&self) -> usize {
        self.clients.read().await.len()
    }
}

#[derive(Clone, Debug)]
struct HickoryResolver {
    resolver: TokioResolver,
    policy: NetworkPolicy,
}

impl HickoryResolver {
    fn new(policy: NetworkPolicy) -> Result<Self, String> {
        let resolver = match TokioResolver::builder_tokio() {
            Ok(builder) => builder.build(),
            Err(_) => TokioResolver::builder_with_config(
                ResolverConfig::default(),
                hickory_resolver::net::runtime::TokioRuntimeProvider::new(),
            )
            .with_options(ResolverOpts::default())
            .build(),
        }
        .map_err(|e| format!("DNS resolver unavailable: {e}"))?;
        Ok(Self { resolver, policy })
    }
}

impl reqwest::dns::Resolve for HickoryResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let resolver = self.resolver.clone();
        let policy = self.policy.clone();
        Box::pin(async move {
            let lookup = resolver.lookup_ip(name.as_str()).await.map_err(|e| {
                Box::new(std::io::Error::other(e.to_string()))
                    as Box<dyn std::error::Error + Send + Sync>
            })?;
            let addrs: Vec<SocketAddr> = lookup
                .iter()
                .filter(|ip| policy.allows_ip(name.as_str(), *ip))
                .map(|ip| SocketAddr::new(ip, 0))
                .collect();
            if addrs.is_empty() {
                return Err(Box::new(std::io::Error::other(
                    crate::download::network_policy::blocked(format!(
                        "SSRF guard: no authorized address for {}",
                        name.as_str()
                    )),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub fn build_client_with_policy(
    config: &ResolvedProxyConfig,
    policy: NetworkPolicy,
) -> Result<Client, String> {
    let resolver = HickoryResolver::new(policy.clone())?;
    let mut builder = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("VibeDownloader/", env!("CARGO_PKG_VERSION")))
        .dns_resolver(Arc::new(resolver))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90));
    match config.mode {
        AppProxyMode::Off => builder = builder.no_proxy(),
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
    if error.to_string().contains('@') {
        "proxy configuration is invalid".to_string()
    } else {
        error.to_string()
    }
}
