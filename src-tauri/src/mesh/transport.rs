//! HTTP transport for peer-to-peer communication.
//! All authenticated requests use Ed25519 signed JWTs.

/// HTTP client for communicating with a paired peer.
pub struct PeerClient {
    client: reqwest::Client,
    base_url: String,
    /// Ed25519 signed token for authenticating requests.
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
    pub async fn auth_post_json(&self, path: &str, body: &impl serde::Serialize) -> Result<serde_json::Value, String> {
        let resp = self.auth_post(path, body).await?;
        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("HTTP error: {}", body))
        }
    }

    /// Authenticated POST helper.
    async fn auth_post(&self, path: &str, body: &impl serde::Serialize) -> Result<reqwest::Response, String> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        req.json(body).send().await.map_err(|e| format!("Network error: {}", e))
    }

    /// Send crown public info update.
    pub async fn send_crown_update(&self, public_info_json: &str) -> Result<(), String> {
        let body: serde_json::Value = serde_json::from_str(public_info_json)
            .map_err(|e| format!("Bad JSON: {}", e))?;
        let resp = self.auth_post("/api/mesh/crown", &body).await?;
        if resp.status().is_success() { Ok(()) } else { Err("Crown update failed".to_string()) }
    }

    /// Send mesh sync payload and receive theirs.
    pub async fn send_sync(
        &self,
        payload: &super::types::MeshSyncPayload,
    ) -> Result<super::types::MeshSyncPayload, String> {
        let resp = self.auth_post("/api/mesh/sync", payload).await?;
        if resp.status().is_success() {
            resp.json().await.map_err(|e| format!("Bad sync response: {}", e))
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
            resp.json().await.map_err(|e| format!("Bad relay response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Relay failed: {}", body))
        }
    }

    /// Request fresh trust declarations from the crown holder.
    pub async fn request_trust_refresh(
        &self,
        peer_ids: &[String],
    ) -> Result<Vec<super::crown::TrustDeclaration>, String> {
        let body = serde_json::json!({ "peer_ids": peer_ids });
        let resp = self.auth_post("/api/mesh/trust/refresh", &body).await?;
        if resp.status().is_success() {
            #[derive(serde::Deserialize)]
            struct RefreshResponse {
                declarations: Vec<super::crown::TrustDeclaration>,
            }
            let parsed: RefreshResponse = resp.json().await
                .map_err(|e| format!("Bad refresh response: {}", e))?;
            Ok(parsed.declarations)
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Trust refresh failed: {}", body))
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
        if resp.status().is_success() { Ok(()) } else { Err("Unpair failed".to_string()) }
    }
}
