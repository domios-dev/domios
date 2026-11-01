//! Tailscale Serve auto-setup — one-click, consent-gated "Enable Web Access".
//!
//! Reachability over a tailnet is best served by `tailscale serve` terminating
//! HTTPS and proxying to this daemon's loopback API port: phones get a clean
//! `https://host.ts.net/` link with no mixed-content or cert prompts. Doing
//! that by hand is a fiddly multi-step CLI dance, so this module automates it —
//! but every mutation is gated behind explicit user confirmation in the UI and
//! is idempotent + defensive so it can never clobber a sibling Domios.
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
#[cfg(not(target_os = "macos"))]
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

#[cfg(target_os = "macos")]
fn tailscale_binary_info() -> TailscaleBinaryInfo {
    let status = macos_status_json();
    TailscaleBinaryInfo {
        installed: status.is_some() || std::path::Path::new("/Applications/Tailscale.app").exists(),
        path: None,
        cli_version: None,
        daemon_version: status
            .as_ref()
            .and_then(|status| status.get("Version"))
            .and_then(serde_json::Value::as_str)
            .map(String::from),
        version_skew: false,
        version_output: None,
        error: status
            .is_none()
            .then(|| "Tailscale LocalAPI not responding".into()),
    }
}

#[cfg(not(target_os = "macos"))]
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
    let (cli_version, daemon_version) = output
        .as_ref()
        .filter(|(ok, _)| *ok)
        .map(|(_, raw)| parse_tailscale_versions(raw))
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

#[cfg(any(not(target_os = "macos"), test))]
fn parse_tailscale_versions(raw: &str) -> (Option<String>, Option<String>) {
    let versions: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|line| semver::Version::parse(line).is_ok())
        .map(String::from)
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
    // Run blocking reqwest work on a dedicated thread because the refresh path already runs inside Tokio.
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
    if resp
        .headers()
        .contains_key(crate::reachability::PEER_ONLY_HEADER)
    {
        return Err("peer-only listener does not serve the Web UI".into());
    }
    let status = resp.status().as_u16();
    let body = resp.text().unwrap_or_default();
    Ok((status, body))
}

pub(crate) fn endpoint_matches_build(url: &str, expected_build: &str) -> bool {
    check_https_health(Some(url), Some(expected_build.to_string())).ok
}

#[cfg(any(target_os = "macos", test))]
mod macos_localapi;
#[cfg(target_os = "macos")]
use macos_localapi::macos_localapi;

#[cfg(target_os = "macos")]
pub(crate) fn macos_status_json() -> Option<serde_json::Value> {
    macos_localapi("/localapi/v0/status")
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

/// The two Domios API ports (prod 7685 / dev 7686) are siblings: a serve
/// listener bound to one while running as the other is the exact #150 footgun.
fn sibling_codeterm_port(port: u16) -> Option<u16> {
    match port {
        7685 => Some(7686),
        7686 => Some(7685),
        _ => None,
    }
}

/// Refuse a public HTTPS port that collides with our own API port or with the
/// sibling Domios port — either would proxy traffic into the wrong daemon.
fn would_clobber_codeterm_port(api_port: u16, https_port: u16) -> bool {
    https_port == api_port || sibling_codeterm_port(api_port) == Some(https_port)
}

/// Idempotent, consent-gated mutation: stand up `tailscale serve` terminating
/// HTTPS and proxying to `http://127.0.0.1:<api_port>`. MUST be called only
/// after the user has explicitly confirmed (the UI spells out the exact
/// commands and only the second click reaches here).
///
/// `https_port` overrides the public port; `None` means try 443 then fall back
/// to 8443. Defends against clobbering a sibling Domios and verifies the
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

    // (2) Refuse a public port that would clobber a Domios API port.
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
mod tests;
