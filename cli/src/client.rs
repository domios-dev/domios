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
    reader: BufReader<interprocess::local_socket::Stream>,
    next_id: u64,
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
        if let Ok(client) = Self::try_connect(socket_path) {
            return Ok(client);
        }
        if !socket_path.ends_with(".sock") {
            let with_sock = format!("{}.sock", socket_path);
            if let Ok(client) = Self::try_connect(&with_sock) {
                return Ok(client);
            }
        }
        if let Some(without) = socket_path.strip_suffix(".sock") {
            if let Ok(client) = Self::try_connect(without) {
                return Ok(client);
            }
        }
        Err(format!(
            "Failed to connect to '{}' (tried with/without .sock)",
            socket_path
        ))
    }

    fn try_connect(socket_path: &str) -> Result<Self, String> {
        let name = socket_path
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| format!("Invalid socket name '{}': {}", socket_path, e))?;

        let stream = interprocess::local_socket::Stream::connect(name)
            .map_err(|e| format!("Failed to connect to '{}': {}", socket_path, e))?;

        Ok(Self {
            reader: BufReader::new(stream),
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
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: self.next_id,
            method,
            params,
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        };
        self.next_id += 1;

        let payload = serde_json::to_string(&req)
            .map_err(|e| SocketCallError::Transport(format!("Serialize error: {}", e)))?;

        let writer = self.reader.get_mut();
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
        let bytes_read = self
            .reader
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

#[cfg(not(target_os = "windows"))]
fn discover_unix_socket() -> Result<String, String> {
    use std::fs;

    // Scan /tmp for codeterm-*.sock files and return the bare name (GenericNamespaced will prepend /tmp/ when connecting)
    if let Ok(entries) = fs::read_dir("/tmp") {
        let mut candidates: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| is_daemon_socket_name(n))
            .collect();
        candidates.sort();
        // A socket file outlives its daemon, and returning a dead one here
        // would shadow the pgrep probe below that still finds the live one.
        candidates.retain(|name| socket_is_live(name));
        if let Some(name) = pick_preferred_socket(&candidates, prefers_dev_socket()) {
            return Ok(name);
        }
    }

    if let Ok(output) = std::process::Command::new("pgrep")
        .arg("-f")
        .arg("codeterm")
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                let sock_name = format!("codeterm-{}.sock", pid);
                let test = sock_name
                    .clone()
                    .to_ns_name::<GenericNamespaced>()
                    .ok()
                    .and_then(|name| interprocess::local_socket::Stream::connect(name).ok());
                if test.is_some() {
                    return Ok(sock_name);
                }
            }
        }
    }

    Err("No CodeTerm socket found. Is CodeTerm running?".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The helper under test only exists off Windows, where /tmp socket files do.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn build_gateway_mutex_files_are_not_daemon_sockets() {
        assert!(!is_daemon_socket_name(
            "codeterm-gateway-mutex-13026-2189552-cargo.sock"
        ));
        assert!(!is_daemon_socket_name(
            "codeterm-gateway-mutex-13026-2189552-rustc.sock"
        ));
        assert!(is_daemon_socket_name("codeterm-49765.sock"));
        assert!(is_daemon_socket_name("codeterm-dev.sock"));
        assert!(!is_daemon_socket_name("codeterm-49765"));
        assert!(!is_daemon_socket_name("other-49765.sock"));
    }

    #[test]
    fn picks_dev_when_dev_preferred() {
        let candidates = vec!["codeterm-1234".to_string(), "codeterm-dev.sock".to_string()];
        assert_eq!(
            pick_preferred_socket(&candidates, true),
            Some("codeterm-dev.sock".to_string())
        );
    }

    #[test]
    fn picks_prod_when_dev_not_preferred() {
        let candidates = vec!["codeterm-1234".to_string(), "codeterm-dev.sock".to_string()];
        assert_eq!(
            pick_preferred_socket(&candidates, false),
            Some("codeterm-1234".to_string())
        );
    }

    #[test]
    fn falls_back_when_only_other_kind_present() {
        let candidates = vec!["codeterm-1234".to_string()];
        assert_eq!(
            pick_preferred_socket(&candidates, true),
            Some("codeterm-1234".to_string())
        );
    }

    #[test]
    fn returns_none_when_nothing_found() {
        let candidates: Vec<String> = vec![];
        assert_eq!(pick_preferred_socket(&candidates, true), None);
        assert_eq!(pick_preferred_socket(&candidates, false), None);
    }

    #[test]
    fn instance_socket_wins_over_plain_dev() {
        let candidates = vec![
            "codeterm-dev.sock".to_string(),
            "codeterm-dev-exp.sock".to_string(),
        ];
        assert_eq!(
            pick_preferred_socket_for(&candidates, true, Some("codeterm-dev-exp")),
            Some("codeterm-dev-exp.sock".to_string())
        );
    }

    #[test]
    fn instance_falls_back_to_dev_when_own_absent() {
        let candidates = vec!["codeterm-dev.sock".to_string()];
        assert_eq!(
            pick_preferred_socket_for(&candidates, true, Some("codeterm-dev-exp")),
            Some("codeterm-dev.sock".to_string())
        );
    }

    #[test]
    fn deterministic_socket_uses_named_instance() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
        let prev_port = std::env::var("CODETERM_API_PORT").ok();
        std::env::set_var("CODETERM_INSTANCE", "handoff");
        std::env::set_var("CODETERM_API_PORT", "7691");

        assert_eq!(
            deterministic_socket_name().as_deref(),
            Some("codeterm-dev-handoff.sock")
        );
        assert_eq!(
            deterministic_socket_resolution().map(|resolved| resolved.source),
            Some(ResolutionSource::CodeTermInstance)
        );

        restore_env("CODETERM_INSTANCE", prev_instance);
        restore_env("CODETERM_API_PORT", prev_port);
    }

    #[test]
    fn deterministic_socket_uses_plain_dev_port() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
        let prev_port = std::env::var("CODETERM_API_PORT").ok();
        std::env::remove_var("CODETERM_INSTANCE");
        std::env::set_var("CODETERM_API_PORT", "7686");

        assert_eq!(
            deterministic_socket_name().as_deref(),
            Some("codeterm-dev.sock")
        );
        assert_eq!(
            deterministic_socket_resolution().map(|resolved| resolved.source),
            Some(ResolutionSource::CodeTermApiPort)
        );

        restore_env("CODETERM_INSTANCE", prev_instance);
        restore_env("CODETERM_API_PORT", prev_port);
    }

    #[test]
    fn deterministic_socket_treats_non_prod_port_as_dev() {
        let _guard = crate::test_env_lock().lock().unwrap();
        let prev_instance = std::env::var("CODETERM_INSTANCE").ok();
        let prev_port = std::env::var("CODETERM_API_PORT").ok();
        std::env::remove_var("CODETERM_INSTANCE");
        std::env::set_var("CODETERM_API_PORT", "7694");

        assert_eq!(
            deterministic_socket_name().as_deref(),
            Some("codeterm-dev.sock")
        );
        assert!(!socket_path_matches_port_pref("codeterm-12345.sock", true));

        restore_env("CODETERM_INSTANCE", prev_instance);
        restore_env("CODETERM_API_PORT", prev_port);
    }

    #[test]
    fn socket_pref_match_dev_pipe_with_dev_pref() {
        assert!(socket_path_matches_port_pref("codeterm-dev.sock", true));
        assert!(socket_path_matches_port_pref("codeterm-dev", true));
    }

    #[test]
    fn socket_pref_match_prod_pid_with_prod_pref() {
        assert!(socket_path_matches_port_pref("codeterm-12345.sock", false));
        assert!(socket_path_matches_port_pref("codeterm-9876", false));
    }

    #[test]
    fn socket_pref_mismatch_when_prod_pipe_but_dev_requested() {
        assert!(!socket_path_matches_port_pref("codeterm-12345.sock", true));
    }

    #[test]
    fn socket_pref_mismatch_when_dev_pipe_but_prod_requested() {
        assert!(!socket_path_matches_port_pref("codeterm-dev.sock", false));
    }

    #[test]
    fn is_dev_socket_does_not_match_pid_paths_with_dev_substring() {
        // `contains("dev")` would have classified `codeterm-99987devops.sock` as dev — guard against that.
        assert!(!is_dev_socket("codeterm-99987devops.sock"));
        assert!(!is_dev_socket("codeterm-developerlaptop.sock"));
        assert!(is_dev_socket("codeterm-dev"));
        assert!(is_dev_socket("codeterm-dev.sock"));
    }

    fn method_not_found() -> SocketCallError {
        SocketCallError::Rpc(RpcError {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        })
    }

    #[test]
    fn version_skew_same_version_call_makes_exactly_one_request() {
        let mut seen: Vec<(String, Option<serde_json::Value>)> = Vec::new();
        let params = serde_json::json!({"tab_id": "t1"});

        let result = call_with_version_fallback(
            |method, params| {
                seen.push((method.to_string(), params));
                Ok(serde_json::json!({"ok": true}))
            },
            "tab.list",
            Some(params.clone()),
        )
        .unwrap();

        assert_eq!(result["ok"], true);
        assert_eq!(seen, vec![("tab.list".to_string(), Some(params))]);
    }

    #[test]
    fn version_skew_typed_error_passes_through_untranslated() {
        let mut calls = 0;
        let outcome = call_with_version_fallback(
            |_, _| {
                calls += 1;
                Err(SocketCallError::Rpc(RpcError {
                    code: crate::protocol::VERSION_SKEW_CODE,
                    message: "client protocol v1, server requires >=2 — run rebuild".into(),
                    data: Some(serde_json::json!({
                        "type": "version_skew",
                        "client_protocol": 1,
                        "server_protocol": 2,
                        "recovery": "rebuild",
                    })),
                }))
            },
            "agent.report",
            None,
        );

        let SocketCallError::Rpc(error) = outcome.unwrap_err() else {
            panic!("the typed error must stay an RPC error");
        };
        assert_eq!(calls, 1, "a skew refusal must not trigger extra requests");
        assert_eq!(error.code, crate::protocol::VERSION_SKEW_CODE);
        let data = error.data.expect("typed data crosses untranslated");
        assert_eq!(data["client_protocol"], 1);
        assert_eq!(data["server_protocol"], 2);
        assert_eq!(data["recovery"], "rebuild");
    }

    #[test]
    fn version_skew_error_renders_the_daemon_message_verbatim() {
        let message = "client protocol v3, server requires >=5 — run ./fix";

        let rendered = SocketCallError::Rpc(RpcError {
            code: crate::protocol::VERSION_SKEW_CODE,
            message: message.into(),
            data: None,
        })
        .to_string();
        assert_eq!(rendered, message);

        let other = SocketCallError::Rpc(RpcError {
            code: -32601,
            message: message.into(),
            data: None,
        })
        .to_string();
        assert_ne!(other, message, "only version skew renders bare");
    }

    #[test]
    fn version_skew_request_envelope_carries_the_protocol_integer() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "tab.list",
            params: None,
            protocol_version: crate::protocol::PROTOCOL_VERSION,
        };
        let wire: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(wire["protocol_version"], crate::protocol::PROTOCOL_VERSION);
    }

    #[test]
    fn version_skew_unknown_method_fallback_names_both_versions() {
        let mut seen: Vec<String> = Vec::new();
        let outcome = call_with_version_fallback(
            |method, _| {
                seen.push(method.to_string());
                if method == "system.identify" {
                    Ok(serde_json::json!({"version": "0.0.1"}))
                } else {
                    Err(method_not_found())
                }
            },
            "tab.list",
            None,
        );

        assert_eq!(
            seen,
            vec!["tab.list".to_string(), "system.identify".to_string()]
        );
        let SocketCallError::Transport(rendered) = outcome.unwrap_err() else {
            panic!("cross-version -32601 becomes a transport explanation");
        };
        assert!(rendered.contains("0.0.1"), "{rendered}");
        assert!(rendered.contains(env!("CARGO_PKG_VERSION")), "{rendered}");
        assert!(rendered.contains("tab.list"), "{rendered}");
    }

    #[test]
    fn version_skew_unknown_method_on_same_version_daemon_stays_method_not_found() {
        let outcome = call_with_version_fallback(
            |method, _| {
                if method == "system.identify" {
                    Ok(serde_json::json!({"version": env!("CARGO_PKG_VERSION")}))
                } else {
                    Err(method_not_found())
                }
            },
            "tab.list",
            None,
        );

        let SocketCallError::Rpc(error) = outcome.unwrap_err() else {
            panic!("same-version -32601 is a real missing method, not skew");
        };
        assert_eq!(error.code, -32601);
    }

    fn restore_env(key: &str, value: Option<String>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
