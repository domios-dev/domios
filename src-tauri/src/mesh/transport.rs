//! Encrypted HTTP transport for peer-to-peer communication.
//!
//! All inter-peer requests use mesh tokens for auth and envelope encryption
//! for payload confidentiality (even on local network without TLS).

use super::types::*;

/// HTTP client for communicating with a paired peer.
pub struct PeerClient {
    client: reqwest::Client,
    base_url: String,
    mesh_token: String,
}

impl PeerClient {
    pub fn new(address: &str, port: u16, mesh_token: &str) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
            base_url: format!("http://{}:{}", address, port),
            mesh_token: mesh_token.to_string(),
        }
    }

    /// Choose the best address for a peer (prefer Tailscale).
    pub fn from_peer(peer: &PairedPeer) -> Option<Self> {
        let addr = peer
            .addresses
            .iter()
            .find(|a| a.is_tailscale)
            .or_else(|| peer.addresses.first())?;
        Some(Self::new(&addr.ip, peer.api_port, &peer.mesh_token))
    }

    /// Ping a peer — returns true if reachable.
    pub async fn ping(&self) -> bool {
        let url = format!("{}/api/mesh/ping", self.base_url);
        matches!(
            self.client.get(&url).send().await,
            Ok(resp) if resp.status().is_success()
        )
    }

    /// Send a pairing request to a peer.
    pub async fn send_pair_request(&self, request: &PairingRequest) -> Result<(), String> {
        let url = format!("{}/api/mesh/pair/request", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Pair request rejected: {}", body))
        }
    }

    /// Send pairing acceptance and read the response (contains requester's encrypted token).
    pub async fn send_pair_accept_and_read(
        &self,
        accept: &PairingAccept,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/mesh/pair/accept", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(accept)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            resp.json::<serde_json::Value>()
                .await
                .map_err(|e| format!("Failed to parse accept response: {}", e))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Pair accept failed: {}", body))
        }
    }

    /// Send pairing confirmation.
    pub async fn send_pair_confirm(&self, confirm: &PairingConfirm) -> Result<(), String> {
        let url = format!("{}/api/mesh/pair/confirm", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(confirm)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Pair confirm failed: {}", body))
        }
    }

    /// Transfer crown to this peer.
    pub async fn send_crown_transfer(&self, crown_token: &str, from_id: &str) -> Result<(), String> {
        let url = format!("{}/api/mesh/crown", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.mesh_token))
            .json(&serde_json::json!({
                "crown_token": crown_token,
                "from_instance_id": from_id,
            }))
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err("Crown transfer failed".to_string())
        }
    }

    /// Notify a peer that we're unpairing.
    pub async fn send_unpair(&self, our_instance_id: &str) -> Result<(), String> {
        let url = format!("{}/api/mesh/peers/{}", self.base_url, our_instance_id);
        let resp = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.mesh_token))
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err("Unpair notification failed".to_string())
        }
    }
}
