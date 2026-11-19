//! HTTP transport for peer-to-peer communication.
//! All authenticated requests use Ed25519 signed JWTs.

use std::{error::Error, sync::LazyLock};

use super::types::{address_tier, PeerAddress};

#[cfg(test)]
mod tailnet_tests;

pub(crate) fn peer_http_origin(address: &str, port: u16) -> String {
    match address.parse::<std::net::Ipv6Addr>() {
        Ok(ip) => format!("http://[{ip}]:{port}"),
        Err(_) => format!("http://{address}:{port}"),
    }
}

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

/// Deadline for reachability probes and authenticated health sync.
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
    /// The connection itself failed (refused, DNS, TLS handshake) — the only
    /// case where the request is provably never written. Nothing was sent.
    NothingSent,
    /// Anything past a successful connect: a timeout, or a connection reset
    /// mid-request. The payload may already have been written and be in
    /// flight on the peer's side, so this is never treated as nothing-sent.
    SubmitUnconfirmed,
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

    /// Classifies a `reqwest::Error` from `.send()`. Only a connect-phase
    /// failure (`is_connect()`) proves the request itself was never written —
    /// that is the sole nothing-sent case. Every other failure (a timeout, or
    /// a connection that was accepted and then reset mid-request) cannot rule
    /// out that the payload already reached the peer, so it must land on the
    /// conservative, retry-unsafe side rather than be assumed harmless.
    fn from_send_error(error: &reqwest::Error) -> Self {
        let kind = if error.is_connect() {
            TransportErrorKind::NothingSent
        } else {
            TransportErrorKind::SubmitUnconfirmed
        };
        Self::new(kind, error.to_string())
    }

    pub fn kind(&self) -> TransportErrorKind {
        self.kind
    }

    /// Stable code a caller can branch on, matching `crate::contract::delivery`.
    pub fn code(&self) -> &'static str {
        match self.kind {
            TransportErrorKind::NothingSent => {
                crate::contract::delivery::MESH_TRANSPORT_NOTHING_SENT
            }
            TransportErrorKind::SubmitUnconfirmed => {
                crate::contract::delivery::MESH_TRANSPORT_SUBMIT_UNCONFIRMED
            }
        }
    }

    /// Mirrors the receiver's `bytes_written` contract: `Some(false)` when
    /// nothing left this hop, `None` when the outcome is genuinely undecidable.
    pub fn bytes_written(&self) -> Option<bool> {
        match self.kind {
            TransportErrorKind::NothingSent => Some(false),
            TransportErrorKind::SubmitUnconfirmed => None,
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

pub fn safe_reqwest_error_detail(error: &reqwest::Error, phase: &str) -> String {
    let causes = std::iter::successors(error.source(), |&source| source.source())
        .map(|source| {
            let text = source.to_string().to_ascii_lowercase();
            if text.contains("refused") {
                "connection_refused"
            } else if text.contains("reset") {
                "connection_reset"
            } else if text.contains("dns")
                || text.contains("name or service")
                || text.contains("resolve")
            {
                "resolution"
            } else if text.contains("timed out") || text.contains("timeout") {
                "timeout"
            } else {
                "other"
            }
        })
        .collect::<Vec<_>>();
    let cause_chain = if causes.is_empty() {
        "none".to_string()
    } else {
        causes.join(",")
    };
    format!(
        "phase={phase};source_depth={};causes={cause_chain}",
        causes.len()
    )
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

#[derive(Debug)]
pub enum SyncFailure {
    Request,
    Refused(u16),
    InvalidReply,
}

/// HTTP client for communicating with a paired peer.
pub struct PeerClient {
    client: reqwest::Client,
    base_url: String,
    relay_alternatives: Vec<String>,
    /// Ed25519 signed token for authenticating requests.
    signed_token: Option<String>,
}

static PEER_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(CLIENT_DEFAULT_TIMEOUT)
        .build()
        .unwrap_or_default()
});

static PEER_STREAM_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
});

static RELAY_HTTP_CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(CLIENT_DEFAULT_TIMEOUT)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
});

pub(super) fn shared_http_client() -> reqwest::Client {
    PEER_HTTP_CLIENT.clone()
}

impl PeerClient {
    pub fn with_signed_token(mut self, signed_token: String) -> Self {
        self.signed_token = Some(signed_token);
        self
    }

    pub fn new_signed_url(base_url: String, signed_token: &str) -> Self {
        Self {
            client: shared_http_client(),
            base_url,
            relay_alternatives: Vec::new(),
            signed_token: Some(signed_token.to_string()),
        }
    }

    /// Create a client with signed JWT auth.
    pub fn new_signed(address: &str, port: u16, signed_token: &str) -> Self {
        Self::new_signed_url(peer_http_origin(address, port), signed_token)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test_without_auth(address: &str, port: u16) -> Self {
        Self {
            client: shared_http_client(),
            base_url: peer_http_origin(address, port),
            relay_alternatives: Vec::new(),
            signed_token: None,
        }
    }

    pub(crate) fn with_relay_alternatives(mut self, alternatives: Vec<String>) -> Self {
        self.relay_alternatives = alternatives;
        self
    }

    fn auth_header(&self) -> Option<(String, String)> {
        self.signed_token
            .as_ref()
            .map(|t| ("Authorization".to_string(), format!("Bearer {}", t)))
    }

    pub async fn auth_get_json(&self, path: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self.client.get(&url);
        if let Some((key, value)) = self.auth_header() {
            request = request.header(&key, &value);
        }
        let response = request
            .send()
            .await
            .map_err(|error| format!("Network error: {error}"))?;
        if response.status().is_success() {
            response
                .json()
                .await
                .map_err(|error| format!("Bad response: {error}"))
        } else {
            Err(format!("HTTP error: {}", response.status()))
        }
    }

    /// Ping a peer — returns true if reachable.
    pub async fn ping(&self) -> bool {
        self.ping_reply().await.is_ok()
    }

    pub async fn ping_reply(&self) -> Result<serde_json::Value, String> {
        let url = format!("{}/api/mesh/ping", self.base_url);
        let mut req = self.client.get(&url).timeout(PING_TIMEOUT);
        if let Some((key, value)) = self.auth_header() {
            req = req.header(key, value);
        }
        let response = req.send().await.map_err(|_| "Ping failed")?;
        if !response.status().is_success() {
            return Err("Ping refused".into());
        }
        response
            .json()
            .await
            .map_err(|_| "Ping reply unreadable".into())
    }

    /// Authenticated POST, returns parsed JSON.
    pub async fn auth_post_json(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<serde_json::Value, String> {
        self.auth_post_json_within(path, body, None).await
    }

    /// Authenticated POST with an explicit failure deadline for peer work that
    /// legitimately outlives the 5s unary default.
    pub async fn auth_post_json_within(
        &self,
        path: &str,
        body: &impl serde::Serialize,
        timeout: Option<std::time::Duration>,
    ) -> Result<serde_json::Value, String> {
        let resp = self.auth_post_within(path, body, timeout).await?;
        if resp.status().is_success() {
            resp.json()
                .await
                .map_err(|e| format!("Bad response: {}", e))
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            if body.is_empty() {
                Err(format!("HTTP error: {status}"))
            } else {
                Err(format!("HTTP error: {status}: {body}"))
            }
        }
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
            .map_err(|error| {
                if error.is_timeout() {
                    let deadline = timeout.unwrap_or(CLIENT_DEFAULT_TIMEOUT);
                    format!("Network error: peer POST {path} timed out after {deadline:?}")
                } else {
                    format!(
                        "Network error: {error} ({})",
                        safe_reqwest_error_detail(&error, "send")
                    )
                }
            })
    }

    /// Send mesh sync payload and receive theirs.
    pub async fn send_sync(
        &self,
        payload: &super::types::MeshSyncPayload,
    ) -> Result<super::types::MeshSyncPayload, SyncFailure> {
        let resp = self
            .auth_post_within("/api/mesh/sync", payload, Some(PING_TIMEOUT))
            .await
            .map_err(|_| SyncFailure::Request)?;
        if resp.status().is_success() {
            resp.json().await.map_err(|_| SyncFailure::InvalidReply)
        } else {
            Err(SyncFailure::Refused(resp.status().as_u16()))
        }
    }

    /// Send relay command. Returns a `TransportError` carrying the failure
    /// kind — never a flattened string — so the caller can tell a hop that
    /// never reached the peer from one whose outcome is genuinely unknown.
    pub async fn send_relay(
        &self,
        cmd: &super::types::RelayCommand,
    ) -> Result<super::types::RelayResponse, TransportError> {
        let client = RELAY_HTTP_CLIENT.as_ref().map_err(|error| {
            TransportError::new(TransportErrorKind::NothingSent, error.to_string())
        })?;
        let mut endpoint = &self.base_url;
        let mut alternatives = self.relay_alternatives.iter();
        loop {
            let request = self
                .relay_request_at(client, endpoint, cmd)
                .map_err(|error| {
                    TransportError::new(TransportErrorKind::NothingSent, error.to_string())
                })?;
            match client.execute(request).await {
                Ok(response) => return decode_relay_response(response).await,
                Err(error) if error.is_connect() => {
                    let Some(next) = alternatives.next() else {
                        return Err(TransportError::from_send_error(&error));
                    };
                    endpoint = next;
                }
                Err(error) => return Err(TransportError::from_send_error(&error)),
            }
        }
    }

    pub fn relay_request(
        &self,
        cmd: &super::types::RelayCommand,
    ) -> Result<reqwest::Request, reqwest::Error> {
        self.relay_request_at(&self.client, &self.base_url, cmd)
    }

    fn relay_request_at(
        &self,
        client: &reqwest::Client,
        endpoint: &str,
        cmd: &super::types::RelayCommand,
    ) -> Result<reqwest::Request, reqwest::Error> {
        let mut request = client.post(format!("{endpoint}/api/mesh/relay"));
        if let Some((key, value)) = self.auth_header() {
            request = request.header(key, value);
        }
        request.timeout(RELAY_TIMEOUT).json(cmd).build()
    }

    /// Open a signed GET against the peer's real endpoint for byte-pipe relaying
    /// (D2 §3). The shared stream client has no total timeout; only connection
    /// setup is bounded so a long-lived SSE stream stays open. Returns the raw
    /// response verbatim (status NOT checked). `path` MUST be caller-validated.
    pub async fn open_relay_stream(&self, path: &str) -> Result<reqwest::Response, String> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = PEER_STREAM_HTTP_CLIENT.get(&url);
        if let Some((k, v)) = self.auth_header() {
            req = req.header(&k, &v);
        }
        req.send()
            .await
            .map_err(|e| format!("Relay stream connect failed: {}", e))
    }
}

async fn decode_relay_response(
    response: reqwest::Response,
) -> Result<super::types::RelayResponse, TransportError> {
    let status = response.status();
    let bytes = response.bytes().await.map_err(|error| {
        TransportError::new(TransportErrorKind::SubmitUnconfirmed, error.to_string())
    })?;
    if status.is_success() {
        let reply =
            serde_json::from_slice::<super::types::RelayResponse>(&bytes).map_err(|error| {
                TransportError::new(
                    TransportErrorKind::SubmitUnconfirmed,
                    format!("Invalid relay reply: {error}"),
                )
            })?;
        if !(200..=599).contains(&reply.status) || reply.ok != (reply.status < 400) {
            return Err(TransportError::new(
                TransportErrorKind::SubmitUnconfirmed,
                "Inconsistent relay reply status",
            ));
        }
        return Ok(reply);
    }
    if status.is_client_error() && status != reqwest::StatusCode::REQUEST_TIMEOUT {
        let text = String::from_utf8_lossy(&bytes);
        let data = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| serde_json::Value::String(text.trim().to_owned()));
        return Ok(super::types::RelayResponse {
            ok: false,
            status: status.as_u16(),
            error: Some(format!("HTTP {status}: {text}")),
            data: Some(data),
        });
    }
    Err(TransportError::new(
        TransportErrorKind::SubmitUnconfirmed,
        format!("Relay returned HTTP {status} without an authoritative command result"),
    ))
}

#[cfg(test)]
mod tests;
