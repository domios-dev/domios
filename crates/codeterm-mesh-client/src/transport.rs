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
    let mut errors: Vec<String> = Vec::new();
    for (idx, addr) in sorted.iter().enumerate() {
        match op(addr).await {
            TryNext::Success(v) => {
                if idx > 0 {
                    tracing::info!(
                        "[mesh/transport] fell back to {} after {} prior failure(s)",
                        addr.ip,
                        idx
                    );
                }
                return Ok(v);
            }
            TryNext::Stop(e) => return Err(e),
            TryNext::Retry(e) => errors.push(format!("{}: {}", addr.ip, e)),
        }
    }
    Err(format!("all addresses failed: {}", errors.join("; ")))
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
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            base_url: format!("http://{}:{}", address, port),
            signed_token: Some(signed_token.to_string()),
        }
    }

    /// Create a client without auth (for pairing requests only).
    pub fn new_unauthenticated(address: &str, port: u16) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            base_url: format!("http://{}:{}", address, port),
            signed_token: None,
        }
    }

    fn auth_header(&self) -> Option<(String, String)> {
        self.signed_token.as_ref().map(|t| {
            ("Authorization".to_string(), format!("Bearer {}", t))
        })
    }

    /// Ping a peer — returns true if reachable.
    pub async fn ping(&self) -> bool {
        let url = format!("{}/api/mesh/ping", self.base_url);
        matches!(self.client.get(&url).send().await, Ok(resp) if resp.status().is_success())
    }

    /// Authenticated POST, returns parsed JSON.
    pub async fn auth_post_json(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<serde_json::Value, String> {
        let resp = self.auth_post(path, body).await?;
        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("HTTP error: {}", body))
        }
    }

    /// Authenticated POST helper.
    async fn auth_post(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<reqwest::Response, String> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        req.json(body).send().await.map_err(|e| format!("Network error: {}", e))
    }

    /// Send mesh sync payload and receive theirs.
    pub async fn send_sync(
        &self,
        payload: &crate::types::MeshSyncPayload,
    ) -> Result<crate::types::MeshSyncPayload, String> {
        let resp = self.auth_post("/api/mesh/sync", payload).await?;
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
        let resp = self.auth_post("/api/mesh/relay", cmd).await?;
        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad relay response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Relay failed: {}", body))
        }
    }

    /// Notify a peer that we're unpairing.
    pub async fn send_unpair(&self, our_instance_id: &str) -> Result<(), String> {
        let url = format!("{}/api/mesh/peers/{}", self.base_url, our_instance_id);
        let mut req = self.client.delete(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        let resp = req.send().await.map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err("Unpair failed".to_string())
        }
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

    /// Get list of panes on this peer.
    pub async fn list_panes(&self) -> Result<Vec<crate::types::PaneInfo>, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes", addr.ip, self.api_port);
            let token = (self.sign_fn)()?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            async move {
                match client.get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let panes: Vec<crate::types::PaneInfo> = resp.json().await.unwrap_or_default();
                        TryNext::Success(panes)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Send text to a pane on this peer.
    pub async fn send_text(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<(), String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/send", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            let body = crate::types::SendTextRequest {
                text: Some(text.to_string()),
                enter,
            };
            async move {
                match client.post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&body)
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => TryNext::Success(()),
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Get output from a pane.
    pub async fn get_output(&self, pane_id: &str) -> Result<String, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/output", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default();
            async move {
                match client.get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                        let output = data.get("output")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        TryNext::Success(output)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }
}