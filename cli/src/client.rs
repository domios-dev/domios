use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::{prelude::*, GenericNamespaced};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
/// Stable provenance labels for values in `identify --json`.
pub enum ResolutionSource {
    #[serde(rename = "CODETERM_DATA_DIR")]
    CodeTermDataDir,
    #[serde(rename = "CODETERM_API_PORT")]
    CodeTermApiPort,
    #[serde(rename = "CODETERM_SOCKET_PATH")]
    CodeTermSocketPath,
    #[serde(rename = "CODETERM_INSTANCE")]
    CodeTermInstance,
    #[serde(rename = "config")]
    Config,
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "scan")]
    Scan,
    #[serde(rename = "absent")]
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSocket {
    pub path: String,
    pub source: ResolutionSource,
}

/// A JSON-RPC client that connects to the CodeTerm socket API.
pub struct SocketClient {
    transport: ClientTransport,
    next_id: u64,
}

enum ClientTransport {
    Socket(BufReader<interprocess::local_socket::Stream>),
    Http {
        client: reqwest::blocking::Client,
        endpoint: String,
        token: reqwest::header::HeaderValue,
        headers: reqwest::header::HeaderMap,
    },
}

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
    protocol_version: u64,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug)]
pub enum SocketCallError {
    Transport(String),
    Rpc(RpcError),
}

struct HttpFallbackConfig {
    base_url: String,
    token: String,
}

enum TransportChoice<T> {
    Socket(T),
    Http(HttpFallbackConfig),
}

fn select_transport<T>(
    socket: Result<T, String>,
    http: impl FnOnce() -> Result<HttpFallbackConfig, String>,
) -> Result<TransportChoice<T>, String> {
    match socket {
        Ok(connection) => Ok(TransportChoice::Socket(connection)),
        Err(socket_error) => http()
            .map(TransportChoice::Http)
            .map_err(|error| {
                format!("CodeTerm socket attempt failed: {socket_error}; loopback HTTP attempt failed: {error}")
            }),
    }
}

fn http_fallback_config() -> Result<HttpFallbackConfig, String> {
    http_fallback_config_from(
        std::env::var("CODETERM_API_URL").ok().as_deref(),
        std::env::var("CODETERM_API_PORT").ok().as_deref(),
        std::env::var("CODETERM_API_TOKEN").ok().as_deref(),
    )
}

fn http_fallback_config_from(
    api_url: Option<&str>,
    api_port: Option<&str>,
    token: Option<&str>,
) -> Result<HttpFallbackConfig, String> {
    let token = token
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "CODETERM_API_TOKEN is not set".to_owned())?;
    let base_url = if let Some(value) = api_url {
        let url =
            reqwest::Url::parse(&value).map_err(|_| "CODETERM_API_URL is invalid".to_owned())?;
        if url.scheme() != "http"
            || url.host_str() != Some("127.0.0.1")
            || url.username() != ""
            || url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("CODETERM_API_URL must target http://127.0.0.1:<port> only".to_owned());
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| "CODETERM_API_URL must include a port".to_owned())?;
        format!("http://127.0.0.1:{port}")
    } else {
        let port = api_port
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| "CODETERM_API_PORT is not set to a valid port".to_owned())?;
        format!("http://127.0.0.1:{port}")
    };
    Ok(HttpFallbackConfig {
        base_url,
        token: token.to_owned(),
    })
}

pub trait RpcClient {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, SocketCallError>;

    fn call(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        self.call_detailed(method, params)
            .map_err(|e| e.to_string())
    }

    fn is_remote(&self) -> bool {
        false
    }
}

impl std::fmt::Display for SocketCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocketCallError::Transport(message) => write!(f, "{message}"),
            SocketCallError::Rpc(err) if crate::protocol::is_version_skew(err.code) => {
                write!(f, "{}", err.message)
            }
            SocketCallError::Rpc(err) => write!(f, "RPC error {}: {}", err.code, err.message),
        }
    }
}

impl SocketClient {
    /// Connect to the CodeTerm socket at the given path.
    ///
    /// On Windows, `socket_path` should be the namespaced name (e.g. `codeterm-1234`),
    /// NOT the full `\\.\pipe\codeterm-1234` path.
    /// Tries both with and without `.sock` suffix for compatibility across versions.
    pub fn connect(socket_path: &str) -> Result<Self, String> {
        let mut candidates = vec![socket_path.to_string()];
        if !socket_path.ends_with(".sock") {
            candidates.push(format!("{}.sock", socket_path));
        }
        if let Some(without) = socket_path.strip_suffix(".sock") {
            candidates.push(without.to_string());
        }
        let mut denied = false;
        for candidate in &candidates {
            match Self::try_connect(candidate) {
                Ok(client) => return Ok(client),
                Err(error) => denied |= error.kind() == std::io::ErrorKind::PermissionDenied,
            }
        }
        Err(connect_failure_message(
            socket_path,
            denied,
            std::env::var("CODETERM_TAB_ID")
                .ok()
                .filter(|tab| !tab.is_empty())
                .as_deref(),
        ))
    }

    fn try_connect(socket_path: &str) -> std::io::Result<Self> {
        let name = socket_path.to_ns_name::<GenericNamespaced>()?;
        let stream = interprocess::local_socket::Stream::connect(name)?;
        Ok(Self::from_socket(stream))
    }

    fn from_socket(stream: interprocess::local_socket::Stream) -> Self {
        Self {
            transport: ClientTransport::Socket(BufReader::new(stream)),
            next_id: 1,
        }
    }

    pub fn connect_local() -> Result<(Self, ResolvedSocket), String> {
        let discovered = discover_socket_resolution();
        let fallback_socket = ResolvedSocket {
            path: discovered
                .as_ref()
                .map(|resolved| resolved.path.clone())
                .unwrap_or_else(|_| std::env::var("CODETERM_SOCKET_PATH").unwrap_or_default()),
            source: discovered
                .as_ref()
                .map(|resolved| resolved.source)
                .unwrap_or(ResolutionSource::Absent),
        };
        let socket_result = discovered
            .and_then(|resolved| Self::connect(&resolved.path).map(|client| (client, resolved)));
        let socket_error = socket_result.as_ref().err().cloned().unwrap_or_default();
        match select_transport(socket_result, http_fallback_config)? {
            TransportChoice::Socket(connection) => Ok(connection),
            TransportChoice::Http(config) => Self::connect_http(config)
                .map(|client| (client, fallback_socket))
                .map_err(|http_error| {
                    format!("CodeTerm socket attempt failed: {socket_error}; loopback HTTP attempt failed: {http_error}")
                }),
        }
    }

    fn connect_http(config: HttpFallbackConfig) -> Result<Self, String> {
        let mut headers = reqwest::header::HeaderMap::new();
        for (env, header) in [
            ("CODETERM_TAB_ID", "x-codeterm-tab-id"),
            ("CODETERM_SESSION_ID", "x-codeterm-session-id"),
            ("CODETERM_SESSION_BINDING_NONCE", "x-codeterm-binding-nonce"),
        ] {
            if let Ok(value) = std::env::var(env) {
                if let Ok(value) = value.parse() {
                    headers.insert(header, value);
                }
            }
        }
        let token = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", config.token))
            .map_err(|_| "CODETERM_API_TOKEN is not a valid HTTP header value".to_owned())?;
        let base_url = config.base_url;
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("failed to create loopback HTTP client: {error}"))?;
        let endpoint = format!("{base_url}/api/cli/rpc");
        let probe = client
            .post(&endpoint)
            .header(reqwest::header::AUTHORIZATION, token.clone())
            .json(&serde_json::json!({
                "method": "system.identify",
                "params": null,
                "protocol_version": crate::protocol::PROTOCOL_VERSION,
            }))
            .send()
            .map_err(|_| "loopback HTTP RPC probe failed".to_owned())?;
        let status = probe.status();
        let value = probe
            .json::<serde_json::Value>()
            .map_err(|_| "loopback HTTP RPC probe returned invalid JSON".to_owned())?;
        http_rpc_result(status, value)
            .map_err(|_| "loopback HTTP RPC probe was refused".to_owned())?;
        Ok(Self {
            transport: ClientTransport::Http {
                client,
                endpoint,
                token,
                headers,
            },
            next_id: 1,
        })
    }

    /// Send a JSON-RPC call and return the result value.
    pub fn call(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        self.call_detailed(method, params)
            .map_err(|e| e.to_string())
    }

    pub fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, SocketCallError> {
        call_with_version_fallback(|m, p| self.call_raw(m, p), method, params)
    }

    fn call_raw(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, SocketCallError> {
        let id = self.next_id;
        self.next_id += 1;
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params: params.clone(),
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        };

        if let ClientTransport::Http {
            client,
            endpoint,
            token,
            headers,
        } = &self.transport
        {
            let mut identity_headers = headers.clone();
            let used_headers: &[&str] = match method {
                "agent.startup_ack" => &["x-codeterm-tab-id", "x-codeterm-binding-nonce"],
                "agent.ack" | "tab.send_text" => &["x-codeterm-tab-id"],
                "agent.report" => &[
                    "x-codeterm-tab-id",
                    "x-codeterm-session-id",
                    "x-codeterm-binding-nonce",
                ],
                _ => &[],
            };
            identity_headers
                .retain(|name, _| used_headers.iter().any(|used| name.as_str() == *used));
            let response = client
                .post(endpoint)
                .header(reqwest::header::AUTHORIZATION, token.clone())
                .headers(identity_headers)
                .json(&serde_json::json!({
                    "method": method,
                    "params": params,
                    "protocol_version": crate::protocol::PROTOCOL_VERSION,
                }))
                .send()
                .map_err(|error| {
                    SocketCallError::Transport(format!("HTTP request failed: {error}"))
                })?;
            let status = response.status();
            let value: serde_json::Value = response.json().map_err(|error| {
                SocketCallError::Transport(format!("invalid HTTP RPC response: {error}"))
            })?;
            return http_rpc_result(status, value);
        }

        let ClientTransport::Socket(reader) = &mut self.transport else {
            unreachable!()
        };
        let payload = serde_json::to_string(&req)
            .map_err(|e| SocketCallError::Transport(format!("Serialize error: {}", e)))?;
        let writer = reader.get_mut();
        writer
            .write_all(payload.as_bytes())
            .map_err(|e| SocketCallError::Transport(format!("Write error: {}", e)))?;
        writer
            .write_all(b"\n")
            .map_err(|e| SocketCallError::Transport(format!("Write error: {}", e)))?;
        writer
            .flush()
            .map_err(|e| SocketCallError::Transport(format!("Flush error: {}", e)))?;

        let mut line = String::new();
        let bytes_read = reader
            .read_line(&mut line)
            .map_err(|e| SocketCallError::Transport(format!("Read error: {}", e)))?;

        if bytes_read == 0 || line.trim().is_empty() {
            return Err(SocketCallError::Transport(format!(
                "daemon closed the socket without responding to '{}' (read {} bytes). \
                 The server-side handler may have panicked during dispatch, or the daemon \
                 restarted mid-request. Retry usually clears it; if it persists, check daemon logs.",
                method, bytes_read
            )));
        }

        let resp: JsonRpcResponse = serde_json::from_str(line.trim()).map_err(|e| {
            let preview: String = line.chars().take(200).collect();
            SocketCallError::Transport(format!(
                "malformed JSON-RPC response from daemon for '{}' ({} bytes, first 200 chars: {:?}): {}",
                method, line.len(), preview, e
            ))
        })?;

        if let Some(err) = resp.error {
            return Err(SocketCallError::Rpc(err));
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

fn http_rpc_result(
    status: reqwest::StatusCode,
    response: serde_json::Value,
) -> Result<serde_json::Value, SocketCallError> {
    if let Some(error) = response.get("error") {
        return Err(SocketCallError::Rpc(RpcError {
            code: error
                .get("code")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(-32603) as i32,
            message: error
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("HTTP RPC failed")
                .to_owned(),
            data: error.get("data").cloned().filter(|value| !value.is_null()),
        }));
    }
    if !status.is_success() {
        return Err(SocketCallError::Transport(format!(
            "HTTP RPC returned status {status}"
        )));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| SocketCallError::Transport("HTTP RPC response is missing result".into()))
}

/// Skew against a NEWER daemon is refused server-side with the typed
/// version-skew error before dispatch, so the same-version path is exactly one
/// request. This fallback covers the one direction the daemon cannot: a newer
/// CLI calling an OLDER daemon that lacks the method entirely (-32601).
fn call_with_version_fallback<F>(
    mut raw: F,
    method: &str,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, SocketCallError>
where
    F: FnMut(&str, Option<serde_json::Value>) -> Result<serde_json::Value, SocketCallError>,
{
    match raw(method, params) {
        Err(SocketCallError::Rpc(error)) if error.code == -32601 => {
            let daemon_version = raw("system.identify", None)
                .ok()
                .and_then(|value| value.get("version")?.as_str().map(str::to_string));
            let Some(daemon_version) = daemon_version else {
                return Err(SocketCallError::Rpc(error));
            };
            let cli_version = env!("CARGO_PKG_VERSION");
            if daemon_version == cli_version {
                return Err(SocketCallError::Rpc(error));
            }
            Err(SocketCallError::Transport(render_protocol_mismatch(
                method,
                cli_version,
                &daemon_version,
            )))
        }
        result => result,
    }
}

fn render_protocol_mismatch(method: &str, cli_version: &str, daemon_version: &str) -> String {
    let ordering = compare_versions(cli_version, daemon_version);
    let direction = match ordering {
        Some(std::cmp::Ordering::Greater) => "CLI is newer than daemon",
        Some(std::cmp::Ordering::Less) => "daemon is newer than CLI",
        _ => "CLI and daemon versions differ",
    };
    format!(
        "CodeTerm CLI/daemon protocol mismatch: {direction} \
         (CLI {cli_version}, daemon {daemon_version}); method '{method}' is unavailable. \
         Restart or update CodeTerm so the running daemon and installed CLI versions match."
    )
}

fn compare_versions(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    fn parse(version: &str) -> Option<Vec<u64>> {
        version
            .split('.')
            .map(|part| part.split('-').next()?.parse().ok())
            .collect()
    }
    Some(parse(left)?.cmp(&parse(right)?))
}

impl RpcClient for SocketClient {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, SocketCallError> {
        SocketClient::call_detailed(self, method, params)
    }
}

/// Discover the CodeTerm socket path.
///
/// 1. Check `CODETERM_SOCKET_PATH` env var, but VERIFY the named pipe / socket
///    is actually live before trusting it. This is the explicit escape hatch.
/// 2. Derive named dev sockets from `CODETERM_INSTANCE` / `CODETERM_API_PORT`.
/// 3. On Windows, scan `\\.\pipe\` for pipes matching `codeterm-*`.
/// 4. On Unix, scan `/tmp/` for `codeterm-*.sock`.
///
/// After a daemon update_apply
///    restart the parent's env var still points at the OLD pid's pipe; the
///    new daemon listens on a fresh pid-suffixed pipe, so any subprocess
///    that blindly trusts the env var ends up with `Failed to connect to
///    'codeterm-{old-pid}.sock'` on every invocation.
pub fn discover_socket() -> Result<String, String> {
    discover_socket_resolution().map(|resolved| resolved.path)
}

pub fn discover_socket_resolution() -> Result<ResolvedSocket, String> {
    let deterministic = deterministic_socket_resolution();
    if let Ok(path) = std::env::var("CODETERM_SOCKET_PATH") {
        if !path.is_empty() {
            let normalized = normalize_socket_path(&path);
            if deterministic
                .as_ref()
                .is_none_or(|resolved| resolved.path == normalized)
                && socket_is_live(&normalized)
            {
                return Ok(ResolvedSocket {
                    path: normalized,
                    source: ResolutionSource::CodeTermSocketPath,
                });
            }
        }
    }

    if let Some(resolved) = deterministic {
        if socket_is_live(&resolved.path) {
            return Ok(resolved);
        }
        return Err(format!(
            "CodeTerm socket '{}' is not live. Check CODETERM_INSTANCE/CODETERM_API_PORT or set CODETERM_SOCKET_PATH.",
            resolved.path
        ));
    }

    #[cfg(target_os = "windows")]
    {
        discover_windows_pipe().map(|path| ResolvedSocket {
            path,
            source: ResolutionSource::Scan,
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        discover_unix_socket().map(|path| ResolvedSocket {
            path,
            source: ResolutionSource::Scan,
        })
    }
}

/// Return true when the named socket exists and is reachable. Used by
/// `discover_socket` to decide whether to trust `CODETERM_SOCKET_PATH`.
fn socket_is_live(name: &str) -> bool {
    normalize_socket_path(name)
        .to_ns_name::<GenericNamespaced>()
        .ok()
        .and_then(|name| interprocess::local_socket::Stream::connect(name).ok())
        .is_some()
}

/// Normalize a socket path to the bare namespaced name that `GenericNamespaced` expects.
///
/// On Windows: strip `\\.\pipe\` prefix.
/// On Unix: strip `/tmp/` prefix (GenericNamespaced adds it back automatically).
fn normalize_socket_path(path: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        if let Some(name) = path.strip_prefix("\\\\.\\pipe\\") {
            return name.to_string();
        }
        if let Some(name) = path.strip_prefix("//./pipe/") {
            return name.to_string();
        }
        path.to_string()
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(name) = path.strip_prefix("/tmp/") {
            return name.to_string();
        }
        path.to_string()
    }
}

/// Whether the caller wants a dev instance.
///
/// Mirrors `read_api_config()`: an explicit non-prod CODETERM_API_PORT targets
/// a dev daemon. Auto-discovery should then prefer a dev socket over any
/// inherited prod socket.
fn prefers_dev_socket() -> bool {
    std::env::var("CODETERM_API_PORT")
        .map(|v| {
            let port = v.trim();
            !port.is_empty() && port != "7685"
        })
        .unwrap_or(false)
}

#[cfg(test)]
fn deterministic_socket_name() -> Option<String> {
    deterministic_socket_resolution().map(|resolved| resolved.path)
}

fn deterministic_socket_resolution() -> Option<ResolvedSocket> {
    if let Some(prefix) = instance_socket_prefix() {
        return Some(ResolvedSocket {
            path: format!("{prefix}.sock"),
            source: ResolutionSource::CodeTermInstance,
        });
    }
    if prefers_dev_socket() {
        return Some(ResolvedSocket {
            path: "codeterm-dev.sock".to_string(),
            source: ResolutionSource::CodeTermApiPort,
        });
    }
    None
}

/// True when the socket name belongs to the dev daemon.
///
/// dev pipes are exactly `codeterm-dev[.sock]`; prod pipes are pid-suffixed
/// (`codeterm-12345.sock`). Match the literal `-dev` segment instead of a
/// loose `contains("dev")` so a custom instance whose pid path happens to
/// have "dev" in it doesn't get misclassified.
fn is_dev_socket(name: &str) -> bool {
    name == "codeterm-dev" || name.starts_with("codeterm-dev.") || name.starts_with("codeterm-dev-")
}

/// True when the socket name matches the requested port preference.
///
/// A mismatch means the inherited SOCKET_PATH points at the wrong daemon —
/// typical when the CLI runs inside a prod Tab and the user has overridden
/// CODETERM_API_PORT to 7686 to target the dev daemon.
#[cfg(test)]
fn socket_path_matches_port_pref(name: &str, prefer_dev: bool) -> bool {
    is_dev_socket(name) == prefer_dev
}

/// The named dev instance socket prefix, e.g. `codeterm-dev-exp`, or None.
/// Sanitized to `[a-z0-9-]+` to mirror the daemon's path derivation.
fn instance_socket_prefix() -> Option<String> {
    match std::env::var("CODETERM_INSTANCE") {
        Ok(s) if !s.trim().is_empty() => {
            let slug: String = s
                .trim()
                .to_lowercase()
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            Some(format!("codeterm-dev-{}", slug))
        }
        _ => None,
    }
}

/// Pick the best socket name from a candidate list given the current preference.
///
/// Pure function — split out so it can be unit-tested without touching the
/// filesystem.
fn pick_preferred_socket(candidates: &[String], prefer_dev: bool) -> Option<String> {
    pick_preferred_socket_for(candidates, prefer_dev, instance_socket_prefix().as_deref())
}

fn pick_preferred_socket_for(
    candidates: &[String],
    prefer_dev: bool,
    instance: Option<&str>,
) -> Option<String> {
    let is_dev = is_dev_socket;

    // 0. A named instance must never bind to another instance's socket.
    if let Some(prefix) = instance {
        if let Some(hit) = candidates
            .iter()
            .find(|n| **n == format!("{prefix}.sock") || **n == *prefix)
        {
            return Some(hit.clone());
        }
    }
    // 1. Prefer matching the requested instance kind (dev vs prod).
    if let Some(hit) = candidates.iter().find(|n| is_dev(n) == prefer_dev) {
        return Some(hit.clone());
    }
    // 2. Fall back to anything we found rather than erroring out.
    candidates.first().cloned()
}

#[cfg(target_os = "windows")]
fn discover_windows_pipe() -> Result<String, String> {
    use std::fs;
    let pipe_dir = "\\\\.\\pipe";
    match fs::read_dir(pipe_dir) {
        Ok(entries) => {
            let mut candidates: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .filter(|n| n.starts_with("codeterm-"))
                .collect();
            candidates.sort();
            if let Some(name) = pick_preferred_socket(&candidates, prefers_dev_socket()) {
                return Ok(name);
            }
            Err("No CodeTerm socket found. Is CodeTerm running?".to_string())
        }
        Err(_) => {
            // Fallback to alternate pipe discovery when read_dir on `\\.\pipe` fails.
            // The PIDs path is best effort only; some Windows builds don't support it cleanly.
            Err("Cannot scan named pipes. Set CODETERM_SOCKET_PATH env var.".to_string())
        }
    }
}

/// Whether a `/tmp` entry can name a CodeTerm daemon socket.
///
/// `build.sh` leaves `codeterm-gateway-mutex-*.sock` behind for its cargo and
/// rustc build locks. Those match the daemon prefix but never speak the CLI
/// protocol, so a leftover one must never be offered as a candidate.
#[cfg(not(target_os = "windows"))]
fn is_daemon_socket_name(name: &str) -> bool {
    name.starts_with("codeterm-")
        && name.ends_with(".sock")
        && !name.starts_with("codeterm-gateway-mutex-")
}

/// Resolve a scanned socket name from raw `/tmp` entry names.
///
/// Liveness, selection and the process probe are injected so the fall-through
/// order can be exercised without a running daemon.
#[cfg(not(target_os = "windows"))]
fn resolve_scanned_socket(
    entry_names: impl Iterator<Item = String>,
    is_live: &dyn Fn(&str) -> bool,
    select: &dyn Fn(&[String]) -> Option<String>,
    probe_running_daemon: &dyn Fn() -> Option<String>,
) -> Result<String, String> {
    let mut candidates: Vec<String> = entry_names.filter(|n| is_daemon_socket_name(n)).collect();
    candidates.sort();
    // A socket file outlives its daemon, and returning a dead one here would
    // shadow the process probe that still finds the live one.
    candidates.retain(|name| is_live(name));
    if let Some(name) = select(&candidates) {
        return Ok(name);
    }

    probe_running_daemon()
        .ok_or_else(|| "No CodeTerm socket found. Is CodeTerm running?".to_string())
}

#[cfg(not(target_os = "windows"))]
fn pgrep_daemon_socket() -> Option<String> {
    let output = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("codeterm")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .map(|pid| format!("codeterm-{}.sock", pid))
        .find(|name| socket_is_live(name))
}

#[cfg(not(target_os = "windows"))]
fn discover_unix_socket() -> Result<String, String> {
    use std::fs;

    // Bare names only: GenericNamespaced prepends /tmp/ when connecting.
    let entry_names: Vec<String> = fs::read_dir("/tmp")
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    resolve_scanned_socket(
        entry_names.into_iter(),
        &socket_is_live,
        &|candidates| pick_preferred_socket(candidates, prefers_dev_socket()),
        &pgrep_daemon_socket,
    )
}

/// An agent sandbox that denies the daemon pipe must not read as a dead daemon.
fn connect_failure_message(socket_path: &str, denied: bool, caller_tab: Option<&str>) -> String {
    let base = format!("Failed to connect to '{socket_path}' (tried with/without .sock)");
    let restart = |tab: &str| format!("from another tab run `codeterm agent restart --tab {tab}`");
    match (denied, caller_tab) {
        (true, Some(tab)) => format!(
            "{base}: access denied. This agent is likely confined by its provider sandbox, launched without CodeTerm's permission bypass; {}",
            restart(tab)
        ),
        (true, None) => {
            format!("{base}: access denied; this process is likely confined by a sandbox")
        }
        (false, Some(tab)) => format!(
            "{base}. If CodeTerm is running, this agent may be confined by its provider sandbox; {}",
            restart(tab)
        ),
        (false, None) => base,
    }
}

#[cfg(test)]
mod tests;
