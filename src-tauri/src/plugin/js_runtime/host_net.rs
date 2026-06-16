//! `host.fetch` backend (host_api v2). Performs an allowlisted HTTP request and
//! returns `{status, headers, body}` as JSON. Host allowlist (`network.allow`)
//! is enforced by the caller via `PluginSandbox::can_reach`.
//!
//! Uses `reqwest::blocking`, which runs on its own internal thread pool, so it
//! is safe to call synchronously from a VM call even when a tokio runtime is
//! active on the calling thread.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchOpts {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn err(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// `host[:port]` of a URL, for the allowlist check. `None` if unparseable.
pub(super) fn host_of(opts_json: &str) -> Option<String> {
    let url = serde_json::from_str::<serde_json::Value>(opts_json)
        .ok()?
        .get("url")?
        .as_str()?
        .to_string();
    let rest = url.split("://").nth(1).unwrap_or(&url);
    rest.split('/').next().map(|s| s.to_string())
}

/// Perform the request described by `opts_json`. The host is already allowlisted.
///
/// Runs on a dedicated OS thread: `reqwest::blocking` panics if constructed while
/// a tokio runtime is active on the calling thread, and plugin VM calls happen on
/// tokio workers. The thread isolates the blocking client from that runtime; the
/// caller blocks on `join`, which is fine — the VM call is synchronous anyway.
pub(super) fn run(opts_json: &str) -> String {
    let owned = opts_json.to_string();
    match std::thread::spawn(move || run_blocking(&owned)).join() {
        Ok(s) => s,
        Err(_) => err("fetch worker panicked"),
    }
}

fn run_blocking(opts_json: &str) -> String {
    let opts: FetchOpts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(&format!("bad fetch opts: {e}")),
    };
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let client = match reqwest::blocking::Client::builder().timeout(timeout).build() {
        Ok(c) => c,
        Err(e) => return err(&format!("client: {e}")),
    };
    let method = reqwest::Method::from_bytes(
        opts.method.as_deref().unwrap_or("GET").to_uppercase().as_bytes(),
    )
    .unwrap_or(reqwest::Method::GET);
    let mut req = client.request(method, &opts.url);
    for (k, v) in &opts.headers {
        req = req.header(k, v);
    }
    if let Some(b) = &opts.body {
        req = req.body(b.clone());
    }
    match req.send() {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let headers: HashMap<String, String> = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            let body = resp.text().unwrap_or_default();
            serde_json::json!({ "status": status, "headers": headers, "body": body }).to_string()
        }
        Err(e) => err(&format!("request failed: {e}")),
    }
}
