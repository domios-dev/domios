//! HTTP transport for peer-to-peer communication.
//! All authenticated requests use Ed25519 signed JWTs.

use super::types::{address_tier, PeerAddress};

/// Outcome of one attempt against a peer address.
///
/// `Retry` means the address failed at the network layer (timeout, refused,
/// DNS, etc.) and the next address in priority order should be tried.
/// `Stop` means the peer responded authoritatively (HTTP 4xx/5xx, parse
/// error) so retrying a different address would just rejection the same
/// way — return the error immediately.
pub enum TryNext<T> {
    Success(T),
    Retry(String),
    Stop(String),
}

/// Walk a peer's addresses in priority order (Tailscale → LAN → public),
/// invoking `op` against each and returning the first success.
///
/// Each address gets its own attempt; connection-class errors fall through
/// to the next, while authoritative responses short-circuit. When every
/// address fails the joined error trail is returned so the caller can log
/// what happened on each route.
pub async fn try_peer<T, F, Fut>(addresses: &[PeerAddress], op: F) -> Result<T, String>
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
                    log::info!(
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

/// HTTP client for communicating with a paired peer.
pub struct PeerClient {
    client: reqwest::Client,
    base_url: String,
    /// Ed25519 signed token for authenticating requests.
    signed_token: Option<String>,
}

impl PeerClient {
    pub fn new_signed_url(base_url: String, signed_token: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            base_url,
            signed_token: Some(signed_token.to_string()),
        }
    }

    /// Create a client with signed JWT auth.
    pub fn new_signed(address: &str, port: u16, signed_token: &str) -> Self {
        Self::new_signed_url(format!("http://{}:{}", address, port), signed_token)
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
        self.signed_token
            .as_ref()
            .map(|t| ("Authorization".to_string(), format!("Bearer {}", t)))
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
            resp.json()
                .await
                .map_err(|e| format!("Bad response: {}", e))
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
        req.json(body)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))
    }

    /// Send mesh sync payload and receive theirs.
    pub async fn send_sync(
        &self,
        payload: &super::types::MeshSyncPayload,
    ) -> Result<super::types::MeshSyncPayload, String> {
        let resp = self.auth_post("/api/mesh/sync", payload).await?;
        if resp.status().is_success() {
            resp.json()
                .await
                .map_err(|e| format!("Bad sync response: {}", e))
        } else {
            Err("Sync failed".to_string())
        }
    }

    /// Send relay command.
    pub async fn send_relay(
        &self,
        cmd: &super::types::RelayCommand,
    ) -> Result<super::types::RelayResponse, String> {
        let resp = self.auth_post("/api/mesh/relay", cmd).await?;
        if resp.status().is_success() {
            resp.json()
                .await
                .map_err(|e| format!("Bad relay response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Relay failed: {}", body))
        }
    }

    /// Open a signed GET against the peer's real endpoint for byte-pipe relaying
    /// (D2 §3). A dedicated client with NO total timeout — the 5s unary client
    /// would kill a long-lived SSE stream; only a connect timeout guards a dead
    /// peer. Returns the raw response verbatim (status NOT checked): the caller
    /// is a dumb pipe and forwards whatever the peer sent. `path` MUST be
    /// caller-validated (whitelist + SSRF guard) before calling.
    pub async fn open_relay_stream(&self, path: &str) -> Result<reqwest::Response, String> {
        let client = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            // off the whitelisted path — forward the redirect verbatim instead.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        let url = format!("{}{}", self.base_url, path);
        let mut req = client.get(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        req.send()
            .await
            .map_err(|e| format!("Relay stream connect failed: {}", e))
    }

    /// Notify a peer that we're unpairing.
    pub async fn send_unpair(&self, our_instance_id: &str) -> Result<(), String> {
        let url = format!("{}/api/mesh/peers/{}", self.base_url, our_instance_id);
        let mut req = self.client.delete(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err("Unpair failed".to_string())
        }
    }
}
