//! HTTP transport for mesh peer-to-peer communication.
//!
//! Provides a `PeerClient` for making authenticated requests to paired peers
//! using Ed25519-signed tokens.

use std::collections::HashMap;

use crate::types::{address_tier, RelayCommand, RelayResponse, PeerAddress};

/// Outcome of one attempt against a peer address.
pub enum TryNext<T> {
    /// Successfully completed
    Success(T),
    /// Network error — try next address
    Retry(String),
    /// Authoritative error (HTTP 4xx/5xx) — stop trying other addresses
    Stop(String),
}

/// Walk a peer's addresses in priority order (Tailscale → LAN → public),
/// invoking `op` against each and returning the first success.
pub async fn try_peer<T, F, Fut>(
    addresses: &[PeerAddress],
    op: F,
) -> Result<T, String>
where
    F: Fn(&PeerAddress) -> Fut,
    Fut: std::future::Future<Output = TryNext<T>>,
{
    if addresses.is_empty() {
        return Err("no addresses".to_string());
    }

    let mut sorted: Vec<&PeerAddress> = addresses.iter().collect();
    sorted.sort_by_key(|a| address_tier(a));

    for (idx, addr) in sorted.iter().enumerate() {
        match op(addr).await {
            TryNext::Success(v) => {
                if idx > 0 {
                    tracing::info!("[mesh/transport] fell back to {} after {} prior failure(s)", addr.ip, idx);
                }
                return Ok(v);
            }
            TryNext::Stop(e) => return Err(e),
            TryNext::Retry(e) => tracing::debug!("[mesh/transport] {} failed: {}", addr.ip, e),
        }
    }

    Err("all addresses failed".to_string())
}

/// HTTP client for communicating with a paired mesh peer.
/// Uses Ed25519 signed tokens for authentication.
pub struct PeerClient {
    client: reqwest::Client,
    base_url: String,
    signed_token: Option<String>,
}

impl PeerClient {
    /// Create a client with signed JWT auth.
    pub fn new_signed(address: &str, port: u16, signed_token: &str) -> Self {
        Self {
            client: Self::http_client(),
            base_url: format!("http://{}:{}", address, port),
            signed_token: Some(signed_token.to_string()),
        }
    }

    /// Create a client without auth (for pairing requests only).
    pub fn new_unauthenticated(address: &str, port: u16) -> Self {
        Self {
            client: Self::http_client(),
            base_url: format!("http://{}:{}", address, port),
            signed_token: None,
        }
    }

    /// Create a configured HTTP client.
    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    }

    /// Build request with optional auth header.
    fn build_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut builder = self.client.request(method, &url);
        if let Some(ref token) = self.signed_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }
        builder
    }

    /// Ping a peer — returns true if reachable.
    pub async fn ping(&self) -> bool {
        matches!(
            self.build_request(reqwest::Method::GET, "/api/mesh/ping")
                .send().await,
            Ok(resp) if resp.status().is_success()
        )
    }

    /// Authenticated POST, returns parsed JSON.
    pub async fn auth_post_json(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<serde_json::Value, String> {
        let resp = self.build_request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("HTTP error: {}", body))
        }
    }

    /// Authenticated DELETE request.
    pub async fn auth_delete(&self, path: &str) -> Result<(), String> {
        let resp = self.build_request(reqwest::Method::DELETE, path)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err("Request failed".to_string())
        }
    }

    /// Send mesh sync payload and receive theirs.
    pub async fn send_sync(
        &self,
        payload: &crate::types::MeshSyncPayload,
    ) -> Result<crate::types::MeshSyncPayload, String> {
        let resp = self.build_request(reqwest::Method::POST, "/api/mesh/sync")
            .json(payload)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad sync response: {}", e))
        } else {
            Err("Sync failed".to_string())
        }
    }

    /// Relay a command to another peer (via this peer's relay endpoint).
    pub async fn relay(
        &self,
        cmd: &RelayCommand,
    ) -> Result<RelayResponse, String> {
        let resp = self.build_request(reqwest::Method::POST, "/api/mesh/relay")
            .json(cmd)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad relay response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Relay failed: {}", body))
        }
    }

    /// Notify a peer that we're unpairing.
    pub async fn send_unpair(&self, our_instance_id: &str) -> Result<(), String> {
        let path = format!("/api/mesh/peers/{}", our_instance_id);
        self.auth_delete(&path).await
    }
}

/// High-level client for interacting with a full mesh peer.
pub struct FullPeerClient {
    addresses: Vec<PeerAddress>,
    api_port: u16,
    sign_fn: Box<dyn Fn() -> Result<String, String> + Send + Sync>,
}

impl FullPeerClient {
    /// Create a client for a peer with addresses and signing callback.
    pub fn new(
        addresses: Vec<PeerAddress>,
        api_port: u16,
        sign_fn: impl Fn() -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            addresses,
            api_port,
            sign_fn: Box::new(sign_fn),
        }
    }

    /// Get the HTTP client.
    fn http_client(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    }

    /// Helper to make an authenticated GET request.
    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}{}", addr.ip, self.api_port, path);
            let token = (self.sign_fn)();
            let client = self.http_client();
            async move {
                match client.get(&url)
                    .header("Authorization", format!("Bearer {}", token?))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        resp.json().await.map_err(|e| e.to_string()).map(TryNext::Success)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Helper to make an authenticated POST request.
    async fn post<T: serde::de::DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}{}", addr.ip, self.api_port, path);
            let token = (self.sign_fn)();
            let client = self.http_client();
            async move {
                match client.post(&url)
                    .header("Authorization", format!("Bearer {}", token?))
                    .json(body)
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        resp.json().await.map_err(|e| e.to_string()).map(TryNext::Success)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Get list of panes on this peer.
    pub async fn list_panes(&self) -> Result<Vec<crate::types::PaneInfo>, String> {
        self.get("/api/panes").await
    }

    /// Send text to a pane on this peer.
    pub async fn send_text(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        let body = crate::types::SendTextRequest {
            text: Some(text.to_string()),
            enter,
        };
        let path = format!("/api/panes/{}/send", pane_id);
        self.post::<(), _>(&path, &body).await
    }

    /// Get output from a pane.
    pub async fn get_output(&self, pane_id: &str) -> Result<String, String> {
        #[derive(serde::Deserialize)]
        struct OutputResponse { output: String }

        let path = format!("/api/panes/{}/output", pane_id);
        let resp: OutputResponse = self.get(&path).await?;
        Ok(resp.output)
    }
}
