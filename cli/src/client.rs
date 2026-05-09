use std::io::{BufRead, BufReader, Write};

use interprocess::local_socket::{prelude::*, GenericNamespaced};
use serde::{Deserialize, Serialize};

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
}

#[derive(Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: Option<serde_json::Value>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl SocketClient {
    /// Connect to the CodeTerm socket at the given path.
    ///
    /// On Windows, `socket_path` should be the namespaced name (e.g. `codeterm-1234`),
    /// NOT the full `\\.\pipe\codeterm-1234` path.
    /// Tries both with and without `.sock` suffix for compatibility across versions.
    pub fn connect(socket_path: &str) -> Result<Self, String> {
        // Try as-is first
        if let Ok(client) = Self::try_connect(socket_path) {
            return Ok(client);
        }
        // Try with .sock suffix if not already present
        if !socket_path.ends_with(".sock") {
            let with_sock = format!("{}.sock", socket_path);
            if let Ok(client) = Self::try_connect(&with_sock) {
                return Ok(client);
            }
        }
        // Try without .sock suffix if present
        if let Some(without) = socket_path.strip_suffix(".sock") {
            if let Ok(client) = Self::try_connect(without) {
                return Ok(client);
            }
        }
        Err(format!("Failed to connect to '{}' (tried with/without .sock)", socket_path))
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
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: self.next_id,
            method,
            params,
        };
        self.next_id += 1;

        let payload =
            serde_json::to_string(&req).map_err(|e| format!("Serialize error: {}", e))?;

        // Write request line
        let writer = self.reader.get_mut();
        writer
            .write_all(payload.as_bytes())
            .map_err(|e| format!("Write error: {}", e))?;
        writer
            .write_all(b"\n")
            .map_err(|e| format!("Write error: {}", e))?;
        writer.flush().map_err(|e| format!("Flush error: {}", e))?;

        // Read response line
        let mut line = String::new();
        let bytes_read = self
            .reader
            .read_line(&mut line)
            .map_err(|e| format!("Read error: {}", e))?;

        if bytes_read == 0 || line.trim().is_empty() {
            return Err(format!(
                "daemon closed the socket without responding to '{}' (read {} bytes). \
                 The server-side handler may have panicked during dispatch, or the daemon \
                 restarted mid-request. Retry usually clears it; if it persists, check daemon logs.",
                method, bytes_read
            ));
        }

        let resp: JsonRpcResponse = serde_json::from_str(line.trim()).map_err(|e| {
            let preview: String = line.chars().take(200).collect();
            format!(
                "malformed JSON-RPC response from daemon for '{}' ({} bytes, first 200 chars: {:?}): {}",
                method, line.len(), preview, e
            )
        })?;

        if let Some(err) = resp.error {
            return Err(format!("RPC error {}: {}", err.code, err.message));
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

/// Discover the CodeTerm socket path.
///
/// 1. Check `CODETERM_SOCKET_PATH` env var, but VERIFY the named pipe / socket
///    is actually live before trusting it. After a daemon update_apply
///    restart the parent's env var still points at the OLD pid's pipe; the
///    new daemon listens on a fresh pid-suffixed pipe, so any subprocess
///    that blindly trusts the env var ends up with `Failed to connect to
///    'codeterm-{old-pid}.sock'` on every invocation.
/// 2. On Windows, scan `\\.\pipe\` for pipes matching `codeterm-*`.
/// 3. On Unix, scan `/tmp/` for `codeterm-*.sock`.
pub fn discover_socket() -> Result<String, String> {
    if let Ok(path) = std::env::var("CODETERM_SOCKET_PATH") {
        if !path.is_empty() {
            let normalized = normalize_socket_path(&path);
            if socket_path_matches_port_pref(&normalized, prefers_dev_socket())
                && socket_is_live(&normalized)
            {
                return Ok(normalized);
            }
            // Either dead pipe (post-restart stale env), or mismatch with
            // CODETERM_API_PORT (e.g. CLI invoked from a prod pane but user
            // set API_PORT=7686 to target dev). Fall through to discovery
            // so the port preference wins.
        }
    }

    #[cfg(target_os = "windows")]
    {
        discover_windows_pipe()
    }

    #[cfg(not(target_os = "windows"))]
    {
        discover_unix_socket()
    }
}

/// Return true when the named socket exists and is reachable. Used by
/// `discover_socket` to decide whether to trust `CODETERM_SOCKET_PATH`.
fn socket_is_live(name: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Named pipes show up in the special \\.\pipe\ directory listing
        // while their server is alive. Cheaper than attempting a full
        // connect() handshake.
        let pipe_dir = "\\\\.\\pipe";
        if let Ok(entries) = std::fs::read_dir(pipe_dir) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy() == name {
                    return true;
                }
            }
        }
        false
    }

    #[cfg(not(target_os = "windows"))]
    {
        let path = std::path::Path::new("/tmp").join(name);
        path.exists()
    }
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
        // GenericNamespaced on Unix prepends /tmp/, so strip it to avoid /tmp//tmp/
        if let Some(name) = path.strip_prefix("/tmp/") {
            return name.to_string();
        }
        path.to_string()
    }
}

/// Whether the caller wants the dev instance.
///
/// Mirrors `read_api_config()`: when CODETERM_API_PORT=7686, the user is
/// targeting the dev daemon. Auto-discovery should then prefer the dev
/// socket pipe over any prod pipe alphabetically encountered first.
fn prefers_dev_socket() -> bool {
    std::env::var("CODETERM_API_PORT")
        .map(|v| v.trim() == "7686")
        .unwrap_or(false)
}

/// True when the socket name matches the requested port preference.
///
/// dev pipes always contain "dev" in the name (e.g. `codeterm-dev.sock`);
/// prod pipes are pid-suffixed (`codeterm-{pid}.sock`). A mismatch means
/// the inherited SOCKET_PATH points at the wrong daemon — typical when
/// the CLI runs inside a prod pane and the user has overridden
/// CODETERM_API_PORT to 7686 to target the dev daemon.
fn socket_path_matches_port_pref(name: &str, prefer_dev: bool) -> bool {
    name.contains("dev") == prefer_dev
}

/// Pick the best socket name from a candidate list given the current preference.
///
/// Pure function — split out so it can be unit-tested without touching the
/// filesystem.
fn pick_preferred_socket(candidates: &[String], prefer_dev: bool) -> Option<String> {
    let is_dev = |name: &str| name.contains("dev");

    // 1. Prefer matching the requested instance.
    if let Some(hit) = candidates.iter().find(|n| is_dev(n) == prefer_dev) {
        return Some(hit.clone());
    }
    // 2. Fall back to anything we found rather than erroring out.
    candidates.first().cloned()
}

#[cfg(target_os = "windows")]
fn discover_windows_pipe() -> Result<String, String> {
    // Scan \\.\pipe\ directory for codeterm-* pipes
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
            // Fallback: try to list pipes via a different method.
            // On some Windows versions, read_dir on \\.\pipe may not work.
            // Try common PIDs approach — not reliable, so just error.
            Err("Cannot scan named pipes. Set CODETERM_SOCKET_PATH env var.".to_string())
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn discover_unix_socket() -> Result<String, String> {
    use std::fs;

    // Scan /tmp for codeterm-*.sock files and return the bare name
    // (GenericNamespaced will prepend /tmp/ when connecting)
    if let Ok(entries) = fs::read_dir("/tmp") {
        let mut candidates: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
            .filter(|n| n.starts_with("codeterm-") && n.ends_with(".sock"))
            .collect();
        candidates.sort();
        if let Some(name) = pick_preferred_socket(&candidates, prefers_dev_socket()) {
            return Ok(name);
        }
    }

    // Fallback: find running CodeTerm process and try connecting by PID
    if let Ok(output) = std::process::Command::new("pgrep").arg("-f").arg("codeterm").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                let sock_name = format!("codeterm-{}.sock", pid);
                let test = sock_name.clone()
                    .to_ns_name::<GenericNamespaced>()
                    .ok()
                    .and_then(|name| {
                        interprocess::local_socket::Stream::connect(name).ok()
                    });
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

    #[test]
    fn picks_dev_when_dev_preferred() {
        let candidates = vec![
            "codeterm-1234".to_string(),
            "codeterm-dev.sock".to_string(),
        ];
        assert_eq!(
            pick_preferred_socket(&candidates, true),
            Some("codeterm-dev.sock".to_string())
        );
    }

    #[test]
    fn picks_prod_when_dev_not_preferred() {
        let candidates = vec![
            "codeterm-1234".to_string(),
            "codeterm-dev.sock".to_string(),
        ];
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
        // The bug from #82: inherited SOCKET_PATH points at prod daemon
        // (pid-suffixed) but user set CODETERM_API_PORT=7686 to hit dev.
        // Must reject the inherited pipe so discovery falls through.
        assert!(!socket_path_matches_port_pref("codeterm-12345.sock", true));
    }

    #[test]
    fn socket_pref_mismatch_when_dev_pipe_but_prod_requested() {
        assert!(!socket_path_matches_port_pref("codeterm-dev.sock", false));
    }
}
