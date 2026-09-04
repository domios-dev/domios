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

/// Floor for trivial control calls; the two requests that can outlive it name
/// their own budget below.
const CLIENT_DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Liveness probe budget. A peer under load answers a trivial GET in seconds
/// rather than milliseconds; declaring it dead at 5s painted a reachable machine
/// offline while its own outbound traffic kept arriving (#319). Paired with the
/// two-miss threshold in `health`, a peer has to stay unreachable for two full
/// rounds before it flips.
const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// A relayed command carries real work on the peer — a build, a test run, a CLI
/// call — so it is bounded by patience with a stalled peer, not by how long the
/// work may legitimately take.
const RELAY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Sender-hop classification for a failed `send_relay`, kept as the reqwest
/// error kind rather than flattened to a string, so a caller can tell
/// nothing-landed from landed-unknown before deciding whether a retry is safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportErrorKind {
    /// Failed before the request reached the peer: connection refused, DNS,
    /// TLS handshake. Nothing was sent.
    NothingSent,
    /// The request timed out waiting on the peer. It may already have been
    /// written and be in flight on the peer's side.
    SubmitUnconfirmed,
    /// The peer answered the relay POST itself with a non-success status.
    PeerRejected,
    /// The peer answered with a success status but the body was unusable.
    BadResponse,
}

/// Sender-hop transport failure for `send_relay`, carrying a stable code and a
/// `bytes_written`-style hint instead of a bare stringified `reqwest::Error`.
#[derive(Debug)]
pub struct TransportError {
    kind: TransportErrorKind,
    detail: String,
}

impl TransportError {
    fn new(kind: TransportErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// Classifies a `reqwest::Error` from `.send()`. A timeout may fire at any
    /// phase of the request, including after the payload was fully written, so
    /// it is the only case treated as landed-unknown rather than nothing-sent.
    fn from_send_error(error: &reqwest::Error) -> Self {
        let kind = if error.is_timeout() {
            TransportErrorKind::SubmitUnconfirmed
        } else {
            TransportErrorKind::NothingSent
        };
        Self::new(kind, error.to_string())
    }

    pub fn kind(&self) -> TransportErrorKind {
        self.kind
    }

    /// Stable code a caller can branch on, matching `codeterm_types::mesh::delivery`.
    pub fn code(&self) -> &'static str {
        match self.kind {
            TransportErrorKind::NothingSent => {
                codeterm_types::mesh::delivery::MESH_TRANSPORT_NOTHING_SENT
            }
            TransportErrorKind::SubmitUnconfirmed => {
                codeterm_types::mesh::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
            }
            TransportErrorKind::PeerRejected => {
                codeterm_types::mesh::delivery::MESH_TRANSPORT_PEER_REJECTED
            }
            TransportErrorKind::BadResponse => {
                codeterm_types::mesh::delivery::MESH_TRANSPORT_BAD_RESPONSE
            }
        }
    }

    /// Mirrors the receiver's `bytes_written` contract: `Some(false)` when
    /// nothing left this hop, `None` when the outcome is genuinely undecidable.
    pub fn bytes_written(&self) -> Option<bool> {
        match self.kind {
            TransportErrorKind::NothingSent => Some(false),
            TransportErrorKind::SubmitUnconfirmed
            | TransportErrorKind::PeerRejected
            | TransportErrorKind::BadResponse => None,
        }
    }

    /// HTTP status a caller can surface: 504 for an undecidable timeout, 502
    /// for every case where the hop itself did not produce a usable answer.
    pub fn http_status(&self) -> u16 {
        match self.kind {
            TransportErrorKind::SubmitUnconfirmed => 504,
            _ => 502,
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail)
    }
}

impl std::error::Error for TransportError {}

impl From<TransportError> for String {
    fn from(error: TransportError) -> String {
        error.to_string()
    }
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
                .timeout(CLIENT_DEFAULT_TIMEOUT)
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
                .timeout(CLIENT_DEFAULT_TIMEOUT)
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
        let req = self.client.get(&url).timeout(PING_TIMEOUT);
        matches!(req.send().await, Ok(resp) if resp.status().is_success())
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
        self.auth_post_within(path, body, None).await
    }

    /// Builds an authenticated POST without sending it, so callers can classify
    /// the send failure themselves instead of it being flattened to a string.
    fn build_post(
        &self,
        path: &str,
        body: &impl serde::Serialize,
        timeout: Option<std::time::Duration>,
    ) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        if let Some(timeout) = timeout {
            req = req.timeout(timeout);
        }
        req.json(body)
    }

    /// `timeout` overrides the client default for this one request.
    async fn auth_post_within(
        &self,
        path: &str,
        body: &impl serde::Serialize,
        timeout: Option<std::time::Duration>,
    ) -> Result<reqwest::Response, String> {
        self.build_post(path, body, timeout)
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))
    }

    /// Send mesh sync payload and receive theirs.
    pub async fn send_sync(
        &self,
        payload: &super::types::MeshSyncPayload,
    ) -> Result<super::types::MeshSyncPayload, String> {
        let resp = self
            .auth_post_within("/api/mesh/sync", payload, Some(PING_TIMEOUT))
            .await?;
        if resp.status().is_success() {
            resp.json()
                .await
                .map_err(|e| format!("Bad sync response: {}", e))
        } else {
            Err("Sync failed".to_string())
        }
    }

    /// Send relay command. Returns a `TransportError` carrying the failure
    /// kind — never a flattened string — so the caller can tell a hop that
    /// never reached the peer from one whose outcome is genuinely unknown.
    pub async fn send_relay(
        &self,
        cmd: &super::types::RelayCommand,
    ) -> Result<super::types::RelayResponse, TransportError> {
        let resp = self
            .build_post("/api/mesh/relay", cmd, Some(RELAY_TIMEOUT))
            .send()
            .await
            .map_err(|e| TransportError::from_send_error(&e))?;
        if resp.status().is_success() {
            resp.json().await.map_err(|e| {
                TransportError::new(
                    TransportErrorKind::BadResponse,
                    format!("Bad relay response: {}", e),
                )
            })
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(TransportError::new(
                TransportErrorKind::PeerRejected,
                format!("Relay failed: {}", body),
            ))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The client default is the floor for trivial control calls. A liveness
    /// probe has to outlive it, or a peer that is merely loaded gets declared
    /// dead — which is what painted a 1ms-away machine offline (#319).
    #[test]
    fn a_probe_outlives_the_default_control_budget() {
        assert!(
            PING_TIMEOUT > CLIENT_DEFAULT_TIMEOUT,
            "a probe that expires with an ordinary control call cannot tell a slow peer from a dead one"
        );
    }

    /// A relayed command carries the peer's actual work, so its budget is about
    /// patience with a stall — never about how long the work may take.
    #[test]
    fn a_relayed_command_outlives_a_probe() {
        assert!(
            RELAY_TIMEOUT > PING_TIMEOUT,
            "relayed work bounded by the probe budget turns a slow peer into a 502"
        );
    }

    /// A connection refused before any request bytes are written must classify
    /// as nothing-sent, never as an undecidable outcome — the caller can safely
    /// retry. Drives a real `reqwest::Error` rather than fabricating one, so the
    /// `is_connect()`/`is_timeout()` classification itself is under test.
    #[tokio::test]
    async fn connection_refused_before_write_is_nothing_sent() {
        // Port 1 (TCPMUX) essentially never has a listener; connecting to it on
        // loopback reliably yields a synchronous connection refusal without
        // depending on a bind/drop race for a free ephemeral port.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("client");
        let error = client
            .post("http://127.0.0.1:1/api/mesh/relay")
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect_err("nothing listens on 127.0.0.1:1");
        assert!(
            error.is_connect(),
            "test setup bug: expected a connect-phase error, got {error:?}"
        );

        let transport_error = TransportError::from_send_error(&error);
        assert_eq!(
            transport_error.code(),
            codeterm_types::mesh::delivery::MESH_TRANSPORT_NOTHING_SENT
        );
        assert_eq!(
            transport_error.bytes_written(),
            Some(false),
            "bytes_written must survive as a definite false, not silently drop"
        );
        assert_ne!(
            transport_error.code(),
            codeterm_types::mesh::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED,
            "nothing-landed must never be confused with landed-unknown"
        );
    }

    /// A timeout after the request was written must classify as landed-unknown
    /// (`submit_unconfirmed`), never as nothing-sent — collapsing the two makes
    /// a retry decision unsafe. The peer here genuinely accepts the connection
    /// (the write lands) and then simply never answers, so the client's own
    /// timeout — not a connection failure — is what fires.
    #[tokio::test]
    async fn timeout_after_write_is_submit_unconfirmed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");

        tokio::spawn(async move {
            if let Ok((_socket, _)) = listener.accept().await {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(300))
            .build()
            .expect("client");
        let error = client
            .post(format!("http://{addr}/api/mesh/relay"))
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect_err("the peer never answers, so the client timeout must fire");
        assert!(
            error.is_timeout(),
            "test setup bug: expected a timeout error, got {error:?}"
        );

        let transport_error = TransportError::from_send_error(&error);
        assert_eq!(
            transport_error.code(),
            codeterm_types::mesh::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
        );
        assert_eq!(
            transport_error.bytes_written(),
            None,
            "the payload may already be in flight — never claim zero bytes here"
        );
        assert_ne!(
            transport_error.code(),
            codeterm_types::mesh::delivery::MESH_TRANSPORT_NOTHING_SENT,
            "landed-unknown must never be confused with nothing-landed"
        );
    }
}
