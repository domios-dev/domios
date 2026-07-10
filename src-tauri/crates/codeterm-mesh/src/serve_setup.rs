//! Tailscale Serve auto-setup — one-click, consent-gated "Enable Web Access".
//!
//! Reachability over a tailnet is best served by `tailscale serve` terminating
//! HTTPS and proxying to this daemon's loopback API port: phones get a clean
//! `https://host.ts.net/` link with no mixed-content or cert prompts. Doing
//! that by hand is a fiddly multi-step CLI dance, so this module automates it —
//! but every mutation is gated behind explicit user confirmation in the UI and
//! is idempotent + defensive so it can never clobber a sibling CodeTerm.
//!
//! Read-side parsing (`tailscale_serve_https_url`) lives here too — it is the
//! inverse of what enable writes, and co-locating keeps the serve concern in
//! one place (it also keeps `discovery.rs` under the file-size limit).

use std::time::Duration;

use super::discovery::{run_with_timeout, tailscale_binary};

/// Hard cap on a single serve mutation (`serve`/`cert`). Generous vs. the read
/// probes because `tailscale cert` provisioning can take a few seconds.
const SERVE_TIMEOUT: Duration = Duration::from_secs(10);
/// Read probe budget for `serve status --json`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Preferred public HTTPS port — phone-friendly, no port suffix in the URL.
const DEFAULT_HTTPS_PORT: u16 = 443;
/// Fallback when 443 can't be bound (common on Linux without privileges).
const FALLBACK_HTTPS_PORT: u16 = 8443;
const HEALTH_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServeReconcileState {
    Ready,
    SetupRequired,
    OperatorBootstrapRequired,
    Conflict,
    Unhealthy,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServeRouteOwnership {
    Owned,
    Conflict,
    Free,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TailscaleBinaryInfo {
    pub installed: bool,
    pub path: Option<String>,
    pub cli_version: Option<String>,
    pub daemon_version: Option<String>,
    pub version_skew: bool,
    pub version_output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServeRouteSnapshot {
    pub host: Option<String>,
    pub https_port: u16,
    pub proxy: Option<String>,
    pub ownership: ServeRouteOwnership,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServeHealthSnapshot {
    pub checked_url: Option<String>,
    pub ok: bool,
    pub status: Option<u16>,
    pub daemon_build: Option<String>,
    pub expected_build: Option<String>,
    pub build_matches: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ServeReconcileSnapshot {
    pub state: ServeReconcileState,
    pub url: Option<String>,
    pub api_port: u16,
    pub desired_https_port: u16,
    pub chosen_https_port: Option<u16>,
    pub operator_bootstrap_required: bool,
    pub bootstrap_command: Option<String>,
    pub binary: TailscaleBinaryInfo,
    pub route: Option<ServeRouteSnapshot>,
    pub routes: Vec<ServeRouteSnapshot>,
    pub conflict: Option<String>,
    pub health: ServeHealthSnapshot,
}

// ── Status (read side) ──────────────────────────────────────────────────

/// Snapshot the UI uses to decide which Web Access affordance to show.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServeStatus {
    /// Tailscale CLI present and responding on this machine.
    pub installed: bool,
    /// Tailscale Serve already terminates HTTPS and proxies to THIS daemon.
    pub active: bool,
    /// The live HTTPS base (`https://host.ts.net[:port]`) when `active`, else None.
    pub https_url: Option<String>,
    /// This node's MagicDNS name (`host.ts.net`) — what the serve URL will be.
    pub self_dns: Option<String>,
    /// Host OS (`std::env::consts::OS`) — the UI needs it to show the macOS-only
    /// `tailscale cert` step in the exact-command confirmation preview.
    pub platform: String,
    pub snapshot: ServeReconcileSnapshot,
}

/// Probe current Tailscale Serve state for `api_port`. Pure read — no mutation.
pub fn probe_serve_status(api_port: u16) -> ServeStatus {
    let snapshot = reconcile_web_serve(api_port, None, None);
    let installed = snapshot.binary.installed;
    let https_url = snapshot.url.clone();
    let self_dns = super::discovery::tailscale_self_dns_name()
        .or_else(|| serve_status_json().as_ref().and_then(serve_config_self_dns));
    ServeStatus {
        installed,
        active: https_url.is_some(),
        https_url,
        self_dns,
        platform: std::env::consts::OS.to_string(),
        snapshot,
    }
}

pub fn reconcile_web_serve(
    api_port: u16,
    desired_https_port: Option<u16>,
    expected_build: Option<String>,
) -> ServeReconcileSnapshot {
    let desired = desired_https_port.unwrap_or(DEFAULT_HTTPS_PORT);
    let binary = tailscale_binary_info();
    let cfg = serve_status_json();
    let routes = cfg
        .as_ref()
        .map(|v| collect_routes(v, api_port))
        .unwrap_or_default();
    let owned = routes
        .iter()
        .find(|r| r.ownership == ServeRouteOwnership::Owned)
        .cloned();
    let chosen = owned
        .as_ref()
        .map(|r| r.https_port)
        .or_else(|| choose_free_https_port(&routes, desired, api_port));
    let url = owned
        .as_ref()
        .and_then(|r| r.host.as_ref().map(|h| format_serve_url(h, r.https_port)));
    let health = check_https_health(url.as_deref(), expected_build.clone());
    let conflict = if owned.is_none() {
        routes
            .iter()
            .find(|r| r.https_port == desired && r.ownership == ServeRouteOwnership::Conflict)
            .map(|r| {
                format!(
                    "https port {} already proxies to {}",
                    r.https_port,
                    r.proxy.as_deref().unwrap_or("another target")
                )
            })
    } else {
        None
    };
    let operator_bootstrap_required = false;
    let state = if !binary.installed {
        ServeReconcileState::SetupRequired
    } else if owned.is_some() && health.ok {
        ServeReconcileState::Ready
    } else if owned.is_some() {
        ServeReconcileState::Unhealthy
    } else if conflict.is_some() && chosen.is_none() {
        ServeReconcileState::Conflict
    } else {
        ServeReconcileState::SetupRequired
    };
    ServeReconcileSnapshot {
        state,
        url,
        api_port,
        desired_https_port: desired,
        chosen_https_port: chosen,
        operator_bootstrap_required,
        bootstrap_command: None,
        binary,
        route: owned,
        routes,
        conflict,
        health,
    }
}

/// Pull this node's `host.ts.net` from a serve-config's `Web` keys (`host:port`).
fn serve_config_self_dns(val: &serde_json::Value) -> Option<String> {
    let web = val.get("Web")?.as_object()?;
    web.keys()
        .find_map(|hp| hp.rsplit_once(':').map(|(h, _)| h.to_string()))
        .filter(|h| h.ends_with(".ts.net"))
}

/// Returns the HTTPS base URL (e.g. `https://host.ts.net`, or
/// `https://host.ts.net:7600` for a non-443 listener) ONLY when Tailscale
/// Serve terminates HTTPS and proxies its root handler to *this* daemon's API
/// port. Returning the URL whenever HTTPS is merely enabled — regardless of
/// where Serve actually proxies — hands users a 502 link when Serve points at a
/// different (e.g. dev) port. Verifying the proxy target is the whole point.
pub fn tailscale_serve_https_url(api_port: u16) -> Option<String> {
    let (host, port) = parse_serve_target(&serve_status_json()?, api_port)?;
    Some(format_serve_url(&host, port))
}

fn format_serve_url(host: &str, port: u16) -> String {
    if port == DEFAULT_HTTPS_PORT {
        format!("https://{host}")
    } else {
        format!("https://{host}:{port}")
    }
}

fn serve_status_json() -> Option<serde_json::Value> {
    // app-bundle shim, which LAUNCHES the GUI app — we must never spawn it.
    #[cfg(target_os = "macos")]
    {
        macos_localapi("/localapi/v0/serve-config")
    }
    #[cfg(not(target_os = "macos"))]
    {
        super::discovery::run_tailscale_json(&["serve", "status", "--json"], PROBE_TIMEOUT)
    }
}

fn collect_routes(val: &serde_json::Value, api_port: u16) -> Vec<ServeRouteSnapshot> {
    let Some(web) = val.get("Web").and_then(|w| w.as_object()) else {
        return Vec::new();
    };
    let mut routes = Vec::new();
    for (host_port, cfg) in web {
        let Some((host, port_s)) = host_port.rsplit_once(':') else {
            continue;
        };
        let Ok(port) = port_s.parse::<u16>() else {
            continue;
        };
        let https_terminated = val
            .get("TCP")
            .and_then(|t| t.get(port_s))
            .and_then(|p| p.get("HTTPS"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        if !https_terminated {
            continue;
        }
        let proxy = cfg
            .get("Handlers")
            .and_then(|h| h.get("/"))
            .and_then(|h| h.get("Proxy"))
            .and_then(|p| p.as_str())
            .map(String::from);
        let ownership = match proxy.as_deref() {
            Some(p) if proxy_targets_local_port(p, api_port) => ServeRouteOwnership::Owned,
            Some(_) => ServeRouteOwnership::Conflict,
            None => ServeRouteOwnership::Unknown,
        };
        routes.push(ServeRouteSnapshot {
            host: Some(host.to_string()),
            https_port: port,
            proxy,
            ownership,
        });
    }
    routes.sort_by_key(|r| r.https_port);
    routes
}

fn choose_free_https_port(
    routes: &[ServeRouteSnapshot],
    desired: u16,
    api_port: u16,
) -> Option<u16> {
    let candidates = [desired, FALLBACK_HTTPS_PORT, 10443, 11443, 12443];
    candidates.into_iter().find(|port| {
        !would_clobber_codeterm_port(api_port, *port)
            && !routes.iter().any(|r| r.https_port == *port)
    })
}

fn tailscale_binary_info() -> TailscaleBinaryInfo {
    let Some(path) = tailscale_binary() else {
        return TailscaleBinaryInfo {
            installed: false,
            path: None,
            cli_version: None,
            daemon_version: None,
            version_skew: false,
            version_output: None,
            error: Some("tailscale binary not found or not responding".into()),
        };
    };
    let output = run_tailscale_capture(&path, &["version"], PROBE_TIMEOUT);
    let version_output = output.as_ref().map(|o| o.1.clone());
    let (cli_version, daemon_version) = version_output
        .as_deref()
        .map(parse_tailscale_versions)
        .unwrap_or((None, None));
    let version_skew =
        cli_version.is_some() && daemon_version.is_some() && cli_version != daemon_version;
    TailscaleBinaryInfo {
        installed: true,
        path: Some(path),
        cli_version,
        daemon_version,
        version_skew,
        version_output,
        error: output.and_then(|(ok, out)| (!ok).then_some(out)),
    }
}

fn parse_tailscale_versions(raw: &str) -> (Option<String>, Option<String>) {
    let versions: Vec<String> = raw
        .lines()
        .filter_map(|line| {
            line.split_whitespace()
                .find(|p| p.chars().next().is_some_and(|c| c.is_ascii_digit()))
        })
        .map(|s| s.trim_matches(',').to_string())
        .collect();
    (versions.first().cloned(), versions.get(1).cloned())
}

fn run_tailscale_capture(bin: &str, args: &[&str], timeout: Duration) -> Option<(bool, String)> {
    let mut c = std::process::Command::new(bin);
    c.args(args);
    codeterm_util::process_util::no_window_std(&mut c);
    run_with_timeout_combined(&mut c, timeout)
}

fn run_with_timeout_combined(
    cmd: &mut std::process::Command,
    timeout: Duration,
) -> Option<(bool, String)> {
    use std::process::Stdio;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(_) => break,
            None if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let output = child.wait_with_output().ok()?;
    let mut raw = String::from_utf8_lossy(&output.stdout).to_string();
    raw.push_str(&String::from_utf8_lossy(&output.stderr));
    Some((output.status.success(), raw))
}

fn check_https_health(url: Option<&str>, expected_build: Option<String>) -> ServeHealthSnapshot {
    let Some(base) = url else {
        return ServeHealthSnapshot {
            checked_url: None,
            ok: false,
            status: None,
            daemon_build: None,
            expected_build,
            build_matches: None,
            error: Some("no owned serve url".into()),
        };
    };
    let health_url = format!("{}/api/health", base.trim_end_matches('/'));
    // reqwest::blocking panics if driven from within a tokio runtime, and this
    // path runs inside the async mesh loop (refresh_local_addresses); isolate it
    // on a dedicated thread that owns no ambient runtime context.
    let fetch_url = health_url.clone();
    let fetched = std::thread::spawn(move || health_get_blocking(&fetch_url))
        .join()
        .unwrap_or_else(|_| Err("health check thread panicked".into()));
    match fetched {
        Ok((status, body)) => {
            let val = serde_json::from_str::<serde_json::Value>(&body).ok();
            let daemon_build = val
                .as_ref()
                .and_then(|v| v.get("build"))
                .and_then(|b| b.as_str())
                .map(String::from);
            let build_matches = expected_build
                .as_ref()
                .and_then(|expected| daemon_build.as_ref().map(|actual| actual == expected));
            ServeHealthSnapshot {
                checked_url: Some(health_url),
                ok: status == 200 && build_matches.unwrap_or(true),
                status: Some(status),
                daemon_build,
                expected_build,
                build_matches,
                error: None,
            }
        }
        Err(e) => ServeHealthSnapshot {
            checked_url: Some(health_url),
            ok: false,
            status: None,
            daemon_build: None,
            expected_build,
            build_matches: None,
            error: Some(e),
        },
    }
}

/// Blocking HTTPS GET of the serve health URL. MUST run on a thread with no
/// ambient tokio runtime — `reqwest::blocking` panics otherwise.
fn health_get_blocking(url: &str) -> Result<(u16, String), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(HEALTH_TIMEOUT)
        .danger_accept_invalid_certs(false)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.text().unwrap_or_default();
    Ok((status, body))
}

/// Read any Tailscale LocalAPI JSON endpoint passively: in-process first (works
/// in dev), then a responsibility-disclaimed helper (signed prod). Returns None
/// when Tailscale isn't running — the loopback read simply fails, so this NEVER
/// wakes/launches the GUI app the way spawning the bundled CLI would.
#[cfg(target_os = "macos")]
pub(crate) fn macos_localapi(path: &str) -> Option<serde_json::Value> {
    localapi_via_proof_file(path).or_else(|| localapi_via_disclaimed_helper(path))
}

/// Full `status --json`-equivalent read passively via the macOS LocalAPI.
#[cfg(target_os = "macos")]
pub(crate) fn macos_status_json() -> Option<serde_json::Value> {
    macos_localapi("/localapi/v0/status")
}

/// This node's Tailnet IPv4 (CGNAT 100.64/10) read passively from the LocalAPI
/// `status` — the macOS replacement for spawning `tailscale ip -4`.
#[cfg(target_os = "macos")]
pub(crate) fn macos_self_tailscale_ip() -> Option<String> {
    macos_status_json()?
        .get("Self")?
        .get("TailscaleIPs")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        .find(|ip| super::discovery::is_tailscale_ip(ip))
        .map(String::from)
}

/// Fetch a LocalAPI path from a responsibility-disclaimed `/bin/sh` helper: the
/// signed `.app` cannot read Tailscale's Group Container in-process (App-Data
/// protection → silent EPERM), but a disclaimed child runs with CLI-tool
/// authority and can. The helper locates the `sameuserproof-<port>-<proof>`
/// token, then curls the LocalAPI — both the protected read and the loopback
/// fetch happen in the allowed context; we only parse the JSON it prints.
#[cfg(target_os = "macos")]
fn localapi_via_disclaimed_helper(path: &str) -> Option<serde_json::Value> {
    let gc = dirs::home_dir()?.join("Library/Group Containers");
    let gc = gc.to_str()?;
    if gc.contains('\'') || !path.starts_with("/localapi/") {
        return None;
    }
    let script = format!(
        "d=$(ls -d '{gc}'/*group.io.tailscale.ipn.macos 2>/dev/null | head -1); \
         [ -n \"$d\" ] || exit 1; \
         f=$(ls \"$d\"/sameuserproof-* 2>/dev/null | head -1); \
         [ -n \"$f\" ] || exit 1; \
         b=$(basename \"$f\"); rest=${{b#sameuserproof-}}; \
         port=${{rest%%-*}}; proof=${{rest#*-}}; \
         exec curl -s -u \":$proof\" \"http://127.0.0.1:$port{path}\""
    );
    let out = codeterm_util::process_util::disclaimed_capture("/bin/sh", &["-c", &script])?;
    let start = out.find('{')?;
    let end = out.rfind('}')?;
    serde_json::from_str(out.get(start..=end)?).ok()
}

/// Fetch a LocalAPI path from the macOS sandboxed Tailscale LocalAPI: discover
/// the `sameuserproof-<port>-<proof>` token file, then a Basic-auth GET (blank
/// user, proof as password) to `http://127.0.0.1:<port><path>`.
#[cfg(target_os = "macos")]
fn localapi_via_proof_file(path: &str) -> Option<serde_json::Value> {
    let gc = dirs::home_dir()?.join("Library/Group Containers");
    for dir in std::fs::read_dir(&gc).ok()?.flatten() {
        if !dir
            .file_name()
            .to_string_lossy()
            .ends_with("group.io.tailscale.ipn.macos")
        {
            continue;
        }
        for f in std::fs::read_dir(dir.path()).ok()?.flatten() {
            let name = f.file_name();
            let name = name.to_string_lossy();
            let Some(rest) = name.strip_prefix("sameuserproof-") else {
                continue;
            };
            let Some((port, proof)) = rest.split_once('-') else {
                continue;
            };
            let Ok(port) = port.parse::<u16>() else {
                continue;
            };
            if let Some(val) = localapi_get(port, proof, path) {
                return Some(val);
            }
        }
    }
    None
}

/// Minimal raw-HTTP GET against the loopback LocalAPI (avoids a reqwest blocking
/// client panicking when called from inside the async mesh loop).
#[cfg(target_os = "macos")]
fn localapi_get(port: u16, proof: &str, path: &str) -> Option<serde_json::Value> {
    use base64::Engine;
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = std::net::TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(PROBE_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(PROBE_TIMEOUT)).ok()?;
    let cred = base64::engine::general_purpose::STANDARD.encode(format!(":{proof}"));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Basic {cred}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    // Slice the JSON object out of the body — robust to chunked framing.
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    serde_json::from_str(body.get(start..=end)?).ok()
}

/// Find the HTTPS-terminating Serve listener whose root `/` handler proxies to
/// this daemon, returning its `(host, port)`. None when Serve isn't terminating
/// HTTPS to us — the honest answer the UI falls back on.
fn parse_serve_target(val: &serde_json::Value, api_port: u16) -> Option<(String, u16)> {
    let web = val.get("Web")?.as_object()?;
    for (host_port, cfg) in web {
        // Key shape: "host.ts.net:443". Split on the LAST colon — the port.
        let (host, port) = host_port.rsplit_once(':')?;
        let https_terminated = val
            .get("TCP")
            .and_then(|t| t.get(port))
            .and_then(|p| p.get("HTTPS"))
            .and_then(|b| b.as_bool())
            .unwrap_or(false);
        if !https_terminated {
            continue;
        }
        let root_proxy = cfg
            .get("Handlers")
            .and_then(|h| h.get("/"))
            .and_then(|h| h.get("Proxy"))
            .and_then(|p| p.as_str());
        if root_proxy.is_some_and(|p| proxy_targets_local_port(p, api_port)) {
            return Some((host.to_string(), port.parse().ok()?));
        }
    }
    None
}

/// True when a Serve proxy target (e.g. `http://127.0.0.1:7685`) points at a
/// loopback host on the given port — i.e. at this daemon.
fn proxy_targets_local_port(proxy: &str, port: u16) -> bool {
    let authority = proxy
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(proxy);
    let authority = authority.split('/').next().unwrap_or(authority);
    let Some((host, p)) = authority.rsplit_once(':') else {
        return false;
    };
    let host_local = matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1");
    host_local && p.parse::<u16>().map(|n| n == port).unwrap_or(false)
}

// ── Enable / Disable (write side, consent-gated) ────────────────────────

/// Outcome of an enable/disable mutation. `code` is a stable machine-readable
/// tag the UI maps to copy; `ok` is the quick success flag.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServeResult {
    pub ok: bool,
    /// One of: `ok` | `already-active` | `already-off` | `tailscale-not-installed`
    /// | `would-clobber-codeterm-port` | `serve-command-failed` | `serve-verify-failed`.
    pub code: String,
    pub https_url: Option<String>,
    pub https_port: u16,
    /// True when 443 was unavailable and we fell back to `FALLBACK_HTTPS_PORT`.
    pub used_fallback: bool,
    pub warning: Option<String>,
    pub snapshot: Option<ServeReconcileSnapshot>,
}

impl ServeResult {
    fn err(code: &str, https_port: u16) -> Self {
        ServeResult {
            ok: false,
            code: code.to_string(),
            https_url: None,
            https_port,
            used_fallback: false,
            warning: None,
            snapshot: None,
        }
    }
}

/// The two CodeTerm API ports (prod 7685 / dev 7686) are siblings: a serve
/// listener bound to one while running as the other is the exact #150 footgun.
fn sibling_codeterm_port(port: u16) -> Option<u16> {
    match port {
        7685 => Some(7686),
        7686 => Some(7685),
        _ => None,
    }
}

/// Refuse a public HTTPS port that collides with our own API port or with the
/// sibling CodeTerm's port — either would proxy traffic into the wrong daemon.
fn would_clobber_codeterm_port(api_port: u16, https_port: u16) -> bool {
    https_port == api_port || sibling_codeterm_port(api_port) == Some(https_port)
}

/// Idempotent, consent-gated mutation: stand up `tailscale serve` terminating
/// HTTPS and proxying to `http://127.0.0.1:<api_port>`. MUST be called only
/// after the user has explicitly confirmed (the UI spells out the exact
/// commands and only the second click reaches here).
///
/// `https_port` overrides the public port; `None` means try 443 then fall back
/// to 8443. Defends against clobbering a sibling CodeTerm and verifies the
/// result before reporting success.
pub fn enable_web_serve(api_port: u16, https_port: Option<u16>) -> ServeResult {
    let requested = https_port;
    let primary = requested.unwrap_or(DEFAULT_HTTPS_PORT);
    let before = reconcile_web_serve(api_port, https_port, None);
    if let Some(url) = before.url {
        return ServeResult {
            ok: true,
            code: "already-active".to_string(),
            https_url: Some(url),
            https_port: before.route.map(|r| r.https_port).unwrap_or(primary),
            used_fallback: false,
            warning: None,
            snapshot: Some(reconcile_web_serve(api_port, https_port, None)),
        };
    }
    let Some(primary) = before.chosen_https_port else {
        return ServeResult::err("serve-port-conflict", primary);
    };

    // (1) No Tailscale → run nothing.
    let Some(bin) = before.binary.path else {
        return ServeResult::err("tailscale-not-installed", primary);
    };

    // (2) Refuse a public port that would clobber a CodeTerm API port.
    if would_clobber_codeterm_port(api_port, primary) {
        return ServeResult::err("would-clobber-codeterm-port", primary);
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(dns) = super::discovery::tailscale_self_dns_name() {
            run_serve_cmd(&bin, &["cert", &dns]);
        }
    }

    // (5) Stand up serve on the primary port.
    let proxy = format!("http://127.0.0.1:{}", api_port);
    let (ok, raw) = run_serve_https_capture(&bin, primary, &proxy);
    if !ok {
        if is_operator_bootstrap_error(&raw) {
            let mut r = ServeResult::err("operator-bootstrap-required", primary);
            r.warning = Some("Run `sudo tailscale set --operator=$USER`, then retry.".into());
            let mut snapshot = reconcile_web_serve(api_port, https_port, None);
            snapshot.state = ServeReconcileState::OperatorBootstrapRequired;
            snapshot.operator_bootstrap_required = true;
            snapshot.bootstrap_command = Some("sudo tailscale set --operator=$USER".into());
            r.snapshot = Some(snapshot);
            return r;
        }
        return ServeResult::err("serve-command-failed", primary);
    }
    let final_port = primary;
    let used_fallback = requested.is_none() && final_port != DEFAULT_HTTPS_PORT;
    let warning =
        used_fallback.then(|| format!("Port 443 is occupied; using {final_port} instead."));

    // (6) Verify Serve actually terminates HTTPS to us now.
    match tailscale_serve_https_url(api_port) {
        Some(url) => ServeResult {
            ok: true,
            code: "ok".to_string(),
            https_url: Some(url),
            https_port: final_port,
            used_fallback,
            warning,
            snapshot: Some(reconcile_web_serve(api_port, Some(final_port), None)),
        },
        None => {
            let mut r = ServeResult::err("serve-verify-failed", final_port);
            r.used_fallback = used_fallback;
            r.warning = warning;
            r
        }
    }
}

/// Tear down the Serve HTTPS listener that currently proxies to this daemon.
/// Idempotent: reports `already-off` when nothing targets us.
pub fn disable_web_serve(api_port: u16) -> ServeResult {
    let Some(bin) = tailscale_binary() else {
        return ServeResult::err("tailscale-not-installed", 0);
    };
    let Some((_, port)) = serve_status_json()
        .as_ref()
        .and_then(|v| parse_serve_target(v, api_port))
    else {
        return ServeResult {
            ok: true,
            code: "already-off".to_string(),
            https_url: None,
            https_port: 0,
            used_fallback: false,
            warning: None,
            snapshot: Some(reconcile_web_serve(api_port, None, None)),
        };
    };
    let https_arg = format!("--https={}", port);
    if !run_serve_cmd(&bin, &["serve", &https_arg, "off"]) {
        return ServeResult::err("serve-command-failed", port);
    }
    if tailscale_serve_https_url(api_port).is_some() {
        return ServeResult::err("serve-verify-failed", port);
    }
    ServeResult {
        ok: true,
        code: "ok".to_string(),
        https_url: None,
        https_port: port,
        used_fallback: false,
        warning: None,
        snapshot: Some(reconcile_web_serve(api_port, Some(port), None)),
    }
}

/// `tailscale serve --bg --https=<port> http://127.0.0.1:<api_port>`.
fn run_serve_https_capture(bin: &str, https_port: u16, proxy: &str) -> (bool, String) {
    let https_arg = format!("--https={}", https_port);
    run_tailscale_capture(bin, &["serve", "--bg", &https_arg, proxy], SERVE_TIMEOUT)
        .unwrap_or((false, "tailscale serve timed out".into()))
}

fn is_operator_bootstrap_error(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("access denied")
        || lower.contains("permission denied")
        || lower.contains("operator")
}

/// Run a Tailscale mutation with the no-console-window spawner and a timeout.
fn run_serve_cmd(bin: &str, args: &[&str]) -> bool {
    let mut c = std::process::Command::new(bin);
    c.args(args);
    codeterm_util::process_util::no_window_std(&mut c);
    matches!(run_with_timeout(&mut c, SERVE_TIMEOUT), Some((true, _)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn serve_json(host_port: &str, https_port: &str, proxy: &str) -> serde_json::Value {
        json!({
            "TCP": { https_port: { "HTTPS": true } },
            "Web": { host_port: { "Handlers": { "/": { "Proxy": proxy } } } }
        })
    }

    fn multi_serve_json() -> serde_json::Value {
        json!({
            "TCP": {
                "443": { "HTTPS": true },
                "8443": { "HTTPS": true },
                "10443": { "HTTPS": true }
            },
            "Web": {
                "host.ts.net:443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:7685" } } },
                "host.ts.net:8443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:7686" } } },
                "host.ts.net:10443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:9000" } } }
            }
        })
    }

    #[test]
    fn target_none_when_no_serve() {
        assert_eq!(parse_serve_target(&json!({}), 7685), None);
    }

    #[test]
    fn url_https_443_targeting_our_port() {
        let v = serve_json("host.ts.net:443", "443", "http://127.0.0.1:7685");
        let (host, port) = parse_serve_target(&v, 7685).unwrap();
        assert_eq!(format_serve_url(&host, port), "https://host.ts.net");
    }

    #[test]
    fn url_https_non_443_keeps_port_suffix() {
        let v = serve_json("host.ts.net:7600", "7600", "http://127.0.0.1:7685");
        let (host, port) = parse_serve_target(&v, 7685).unwrap();
        assert_eq!(format_serve_url(&host, port), "https://host.ts.net:7600");
    }

    #[test]
    fn target_none_when_proxy_targets_different_port() {
        // Serve points at the dev port (7686) but we are the prod daemon (7685):
        // advertising the HTTPS URL here would hand the user a 502 link.
        let v = serve_json("host.ts.net:443", "443", "http://127.0.0.1:7686");
        assert_eq!(parse_serve_target(&v, 7685), None);
    }

    #[test]
    fn target_none_when_https_not_terminated() {
        let v = json!({
            "TCP": { "443": { "HTTPS": false } },
            "Web": { "host.ts.net:443": { "Handlers": { "/": { "Proxy": "http://127.0.0.1:7685" } } } }
        });
        assert_eq!(parse_serve_target(&v, 7685), None);
    }

    #[test]
    fn proxy_target_matches_loopback_variants() {
        assert!(proxy_targets_local_port("http://127.0.0.1:7685", 7685));
        assert!(proxy_targets_local_port("http://localhost:7685", 7685));
        assert!(!proxy_targets_local_port("http://127.0.0.1:7686", 7685));
        assert!(!proxy_targets_local_port("http://10.0.0.5:7685", 7685));
        assert!(!proxy_targets_local_port("garbage", 7685));
    }

    #[test]
    fn self_dns_from_serve_config_web_key() {
        let v = serve_json("host.ts.net:443", "443", "http://127.0.0.1:7685");
        assert_eq!(serve_config_self_dns(&v).as_deref(), Some("host.ts.net"));
        assert_eq!(serve_config_self_dns(&json!({})), None);
        // Non-tailnet host keys are ignored.
        let bad = serve_json("localhost:8080", "8080", "http://127.0.0.1:7685");
        assert_eq!(serve_config_self_dns(&bad), None);
    }

    #[test]
    fn collect_routes_marks_owned_and_conflicts_without_dropping_any() {
        let routes = collect_routes(&multi_serve_json(), 7686);
        assert_eq!(routes.len(), 3);
        assert_eq!(routes[0].https_port, 443);
        assert_eq!(routes[0].ownership, ServeRouteOwnership::Conflict);
        assert_eq!(routes[1].https_port, 8443);
        assert_eq!(routes[1].ownership, ServeRouteOwnership::Owned);
        assert_eq!(routes[2].https_port, 10443);
        assert_eq!(routes[2].ownership, ServeRouteOwnership::Conflict);
    }

    #[test]
    fn choose_free_https_port_preserves_existing_mappings() {
        let routes = collect_routes(&multi_serve_json(), 7685);
        assert_eq!(choose_free_https_port(&routes, 443, 7685), Some(11443));
    }

    #[test]
    fn choose_free_https_port_refuses_own_and_sibling_api_ports() {
        let routes = Vec::new();
        assert_eq!(choose_free_https_port(&routes, 7686, 7685), Some(8443));
        assert_eq!(choose_free_https_port(&routes, 7685, 7685), Some(8443));
    }

    #[test]
    fn parse_tailscale_versions_detects_client_daemon_skew() {
        let (cli, daemon) = parse_tailscale_versions(
            "1.82.5\n  tailscale commit: abc\n  other commit: def\n1.80.3\n",
        );
        assert_eq!(cli.as_deref(), Some("1.82.5"));
        assert_eq!(daemon.as_deref(), Some("1.80.3"));
        assert_ne!(cli, daemon);
    }

    #[test]
    fn operator_bootstrap_error_is_explicit() {
        assert!(is_operator_bootstrap_error(
            "Access denied: user is not allowed to modify tailscale serve config"
        ));
        assert!(is_operator_bootstrap_error(
            "set --operator=rollacode before using this command"
        ));
        assert!(!is_operator_bootstrap_error("connection refused"));
    }

    #[test]
    fn sibling_ports_pair_up() {
        assert_eq!(sibling_codeterm_port(7685), Some(7686));
        assert_eq!(sibling_codeterm_port(7686), Some(7685));
        assert_eq!(sibling_codeterm_port(9000), None);
    }

    #[test]
    fn clobber_guard_rejects_own_and_sibling_ports() {
        // Public port == our own API port.
        assert!(would_clobber_codeterm_port(7685, 7685));
        // Public port == sibling CodeTerm's API port (the #150 footgun).
        assert!(would_clobber_codeterm_port(7685, 7686));
        assert!(would_clobber_codeterm_port(7686, 7685));
        // 443 / 8443 against a CodeTerm api_port are fine.
        assert!(!would_clobber_codeterm_port(7685, 443));
        assert!(!would_clobber_codeterm_port(7685, 8443));
        // Non-sibling custom port: only self-collision is rejected.
        assert!(would_clobber_codeterm_port(9000, 9000));
        assert!(!would_clobber_codeterm_port(9000, 7686));
    }
}
