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
        self.reader
            .read_line(&mut line)
            .map_err(|e| format!("Read error: {}", e))?;

        let resp: JsonRpcResponse =
            serde_json::from_str(line.trim()).map_err(|e| format!("Parse response error: {}", e))?;

        if let Some(err) = resp.error {
            return Err(format!("RPC error {}: {}", err.code, err.message));
        }

        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

/// Discover the CodeTerm socket path.
///
/// 1. Check `CODETERM_SOCKET_PATH` env var
/// 2. On Windows, scan `\\.\pipe\` for pipes matching `codeterm-*`
/// 3. On Unix, scan `/tmp/` for `codeterm-*.sock`
pub fn discover_socket() -> Result<String, String> {
    // Check env var first
    if let Ok(path) = std::env::var("CODETERM_SOCKET_PATH") {
        if !path.is_empty() {
            // The env var contains the display path (e.g. \\.\pipe\codeterm-1234).
            // We need to extract just the namespaced name for interprocess.
            return Ok(normalize_socket_path(&path));
        }
    }

    // Auto-discovery
    #[cfg(target_os = "windows")]
    {
        discover_windows_pipe()
    }

    #[cfg(not(target_os = "windows"))]
    {
        discover_unix_socket()
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
}
