//! High-level Pane API client.
//!
//! Provides methods for interacting with panes on full mesh peers,
//! handling address selection and authentication.

use std::sync::Arc;
use parking_lot::Mutex;
use futures_core::Stream;
use reqwest::Client;
use codeterm_mesh_client::{
    types::{PeerAddress, PaneInfo, address_tier},
    transport::{try_peer, TryNext},
};

use crate::types::{
    SendTextRequest, SendTextResponse,
    ExecRequest, ExecResponse,
    ResizePaneRequest,
    PaneStreamEvent,
};

/// High-level client for interacting with panes on a full mesh peer.
pub struct PaneApiClient {
    /// Addresses of the peer (tried in priority order).
    addresses: Vec<PeerAddress>,
    /// API port on the peer.
    api_port: u16,
    /// Signing function to create authenticated tokens.
    sign_fn: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
    /// HTTP client.
    client: Client,
}

impl PaneApiClient {
    /// Create a new PaneApiClient.
    pub fn new(
        addresses: Vec<PeerAddress>,
        api_port: u16,
        sign_fn: impl Fn() -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            addresses,
            api_port,
            sign_fn: Arc::new(sign_fn),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Update the addresses (useful when peer announces new addresses).
    pub fn set_addresses(&mut self, addresses: Vec<PeerAddress>) {
        self.addresses = addresses;
    }

    /// Get the best available address (Tailscale > LAN > public).
    pub fn best_address(&self) -> Option<&PeerAddress> {
        self.addresses.iter().min_by_key(|a| address_tier(a))
    }

    /// List all panes on this peer.
    pub async fn list_panes(&self) -> Result<Vec<PaneInfo>, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes", addr.ip, self.api_port);
            let token = (self.sign_fn)()?;
            let client = self.client.clone();
            async move {
                match client.get(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let panes: Vec<PaneInfo> = resp.json().await.unwrap_or_default();
                        TryNext::Success(panes)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Send text to a pane.
    pub async fn send_text(
        &self,
        pane_id: &str,
        text: &str,
        enter: bool,
    ) -> Result<SendTextResponse, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/send", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let body = SendTextRequest {
                text: Some(text.to_string()),
                enter,
                batch: None,
                raw: false,
                verify: None,
            };
            let client = self.client.clone();
            async move {
                match client.post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&body)
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: SendTextResponse = resp.json().await.unwrap_or(SendTextResponse {
                            ok: true,
                            verified: false,
                            echo_observed: false,
                            submit_observed: false,
                            elapsed_ms: None,
                        });
                        TryNext::Success(data)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Execute a command in a pane and wait for output.
    pub async fn exec(
        &self,
        pane_id: &str,
        command: &str,
        timeout_ms: Option<u64>,
    ) -> Result<ExecResponse, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/exec", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let body = ExecRequest {
                command: command.to_string(),
                timeout_ms,
                strip_ansi: Some(true),
            };
            let client = self.client.clone();
            async move {
                match client.post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&body)
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => {
                        let data: ExecResponse = resp.json().await.map_err(|e| e.to_string())?;
                        TryNext::Success(data)
                    }
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Get current output from a pane.
    pub async fn get_output(
        &self,
        pane_id: &str,
        offset: Option<usize>,
    ) -> Result<String, String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/output", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let client = self.client.clone();
            async move {
                let mut req = client.get(&url);
                req = req.header("Authorization", format!("Bearer {}", token));
                if let Some(o) = offset {
                    req = req.query(&[("offset", o)]);
                }
                match req.send().await {
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

    /// Get an SSE stream of pane output.
    pub async fn stream_output(
        &self,
        pane_id: &str,
    ) -> Result<impl Stream<Item = Result<PaneStreamEvent, String>>, String> {
        // For SSE streaming, we need to use try_peer differently
        // This returns a stream that can be used with axum or tokio
        let addr = self.best_address().ok_or("no addresses")?;
        let url = format!("http://{}:{}/api/panes/{}/output/stream", addr.ip, self.api_port, pane_id);
        let token = (self.sign_fn)()?;

        let client = self.client.clone();
        let response = client.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }

        // Create an SSE stream from the response
        use futures_core::Stream as StreamTrait;
        use bytes::Bytes;
        use tokio_stream::StreamExt;

        let stream = response.bytes_stream()
            .map(|chunk| chunk.map_err(|e| e.to_string()))
            .map(|chunk| {
                // Parse SSE events
                // In a real implementation, we'd parse the SSE format properly
                // For now, return the raw bytes
                chunk
            });

        Ok(stream)
    }

    /// Resize a pane.
    pub async fn resize(
        &self,
        pane_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/resize", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let body = ResizePaneRequest { cols, rows };
            let client = self.client.clone();
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

    /// Focus a pane.
    pub async fn focus(&self, pane_id: &str) -> Result<(), String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/focus", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let client = self.client.clone();
            async move {
                match client.post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => TryNext::Success(()),
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }

    /// Close a pane.
    pub async fn close(&self, pane_id: &str) -> Result<(), String> {
        try_peer(&self.addresses, |addr| {
            let url = format!("http://{}:{}/api/panes/{}/close", addr.ip, self.api_port, pane_id);
            let token = (self.sign_fn)()?;
            let client = self.client.clone();
            async move {
                match client.post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await
                {
                    Ok(resp) if resp.status().is_success() => TryNext::Success(()),
                    Ok(resp) => TryNext::Stop(format!("HTTP {}", resp.status())),
                    Err(e) => TryNext::Retry(e.to_string()),
                }
            }
        }).await
    }
}

/// Builder for creating PaneApiClient.
pub struct PaneApiClientBuilder {
    addresses: Vec<PeerAddress>,
    api_port: u16,
}

impl PaneApiClientBuilder {
    pub fn new() -> Self {
        Self {
            addresses: Vec::new(),
            api_port: 7685,
        }
    }

    pub fn addresses(mut self, addrs: Vec<PeerAddress>) -> Self {
        self.addresses = addrs;
        self
    }

    pub fn api_port(mut self, port: u16) -> Self {
        self.api_port = port;
        self
    }

    pub fn build<F>(self, sign_fn: F) -> PaneApiClient
    where
        F: Fn() -> Result<String, String> + Send + Sync + 'static,
    {
        PaneApiClient::new(self.addresses, self.api_port, sign_fn)
    }
}

impl Default for PaneApiClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}