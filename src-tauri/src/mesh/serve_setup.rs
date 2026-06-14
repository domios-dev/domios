//! Tailscale Serve auto-setup — one-click, consent-gated "Enable Web Access".
//!
//! Reachability over a tailnet is best served by `tailscale serve` terminating
//! HTTPS and proxying to this daemon's loopback API port: phones get a clean
//! `https://host.ts.net/web/` link with no mixed-content or cert prompts. Doing
//! that by hand is a fiddly multi-step CLI dance, so this module automates it —
//! but every mutation is gated behind explicit user confirmation in the UI and
//! is idempotent + defensive so it can never clobber a sibling CodeTerm.
//!
//! Read-side parsing (`tailscale_serve_https_url`) lives here too — it is the
//! inverse of what enable writes, and co-locating keeps the serve concern in
//! one place (it also keeps `discovery.rs` under the file-size limit).

use std::time::Duration;

use super::discovery::{run_tailscale_json, run_with_timeout, tailscale_binary};

/// Hard cap on a single serve mutation (`serve`/`cert`). Generous vs. the read
/// probes because `tailscale cert` provisioning can take a few seconds.
const SERVE_TIMEOUT: Duration = Duration::from_secs(10);
/// Read probe budget for `serve status --json`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Preferred public HTTPS port — phone-friendly, no port suffix in the URL.
const DEFAULT_HTTPS_PORT: u16 = 443;
/// Fallback when 443 can't be bound (common on Linux without privileges).
const FALLBACK_HTTPS_PORT: u16 = 8443;

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
}

/// Probe current Tailscale Serve state for `api_port`. Pure read — no mutation.
pub fn probe_serve_status(api_port: u16) -> ServeStatus {
    let installed = tailscale_binary().is_some();
    let https_url = tailscale_serve_https_url(api_port);
    ServeStatus {
        installed,
        active: https_url.is_some(),
        https_url,
        self_dns: super::discovery::tailscale_self_dns_name(),
        platform: std::env::consts::OS.to_string(),
    }
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
    run_tailscale_json(&["serve", "status", "--json"], PROBE_TIMEOUT)
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

    // (1) No Tailscale → run nothing.
    let Some(bin) = tailscale_binary() else {
        return ServeResult::err("tailscale-not-installed", primary);
    };

    // (3) Already serving us on some HTTPS port → idempotent short-circuit.
    if let Some(url) = tailscale_serve_https_url(api_port) {
        let port = serve_status_json()
            .as_ref()
            .and_then(|v| parse_serve_target(v, api_port))
            .map(|(_, p)| p)
            .unwrap_or(primary);
        return ServeResult {
            ok: true,
            code: "already-active".to_string(),
            https_url: Some(url),
            https_port: port,
            used_fallback: false,
            warning: None,
        };
    }

    // (2) Refuse a public port that would clobber a CodeTerm API port.
    if would_clobber_codeterm_port(api_port, primary) {
        return ServeResult::err("would-clobber-codeterm-port", primary);
    }

    // (4) macOS provisions a cert BEFORE serve; non-fatal if it fails (the cert
    //     may already exist, or serve will surface the real error). cfg-gated so
    //     Windows/Linux never spawn it.
    #[cfg(target_os = "macos")]
    {
        if let Some(dns) = super::discovery::tailscale_self_dns_name() {
            run_serve_cmd(&bin, &["cert", &dns]);
        }
    }

    // (5) Stand up serve on the primary port.
    let proxy = format!("http://127.0.0.1:{}", api_port);
    let (final_port, used_fallback, warning) = if run_serve_https(&bin, primary, &proxy) {
        (primary, false, None)
    } else if requested.is_none() && primary == DEFAULT_HTTPS_PORT {
        // (3) 443 failed → fall back to 8443 with a warning.
        if would_clobber_codeterm_port(api_port, FALLBACK_HTTPS_PORT) {
            return ServeResult::err("would-clobber-codeterm-port", FALLBACK_HTTPS_PORT);
        }
        if run_serve_https(&bin, FALLBACK_HTTPS_PORT, &proxy) {
            (
                FALLBACK_HTTPS_PORT,
                true,
                Some(format!(
                    "Port 443 was unavailable; using {} instead.",
                    FALLBACK_HTTPS_PORT
                )),
            )
        } else {
            return ServeResult::err("serve-command-failed", primary);
        }
    } else {
        return ServeResult::err("serve-command-failed", primary);
    };

    // (6) Verify Serve actually terminates HTTPS to us now.
    match tailscale_serve_https_url(api_port) {
        Some(url) => ServeResult {
            ok: true,
            code: "ok".to_string(),
            https_url: Some(url),
            https_port: final_port,
            used_fallback,
            warning,
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
    }
}

/// `tailscale serve --bg --https=<port> http://127.0.0.1:<api_port>`.
fn run_serve_https(bin: &str, https_port: u16, proxy: &str) -> bool {
    let https_arg = format!("--https={}", https_port);
    run_serve_cmd(bin, &["serve", "--bg", &https_arg, proxy])
}

/// Run a Tailscale mutation with the no-console-window spawner and a timeout.
fn run_serve_cmd(bin: &str, args: &[&str]) -> bool {
    let mut c = std::process::Command::new(bin);
    c.args(args);
    crate::process_util::no_window_std(&mut c);
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
