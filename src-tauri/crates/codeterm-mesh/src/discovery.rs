//! Self endpoint detection only. New peers come from the account registry, never from
//! LAN beacons, mDNS browsing or Tailnet-wide enumeration.

use std::sync::Arc;

use parking_lot::Mutex;

use super::types::PeerAddress;
use super::MeshManager;

/// Cadence of authenticated sync with already-paired peers.
pub const PROACTIVE_SYNC_INTERVAL_SECS: u64 = 60;

// ── Tailscale / Local IP ───────────────────────────────────────────────

/// Hard cap on how long any single Tailscale CLI probe is allowed to run.
#[cfg(not(target_os = "macos"))]
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Binary lookup order for the Tailscale CLI: bare name (resolved via PATH)
/// first, then the common absolute install locations (macOS app bundle,
/// Homebrew, Linux package). Shared by every probe + the serve-setup module.
pub(crate) const TAILSCALE_CANDIDATES: [&str; 5] = [
    "tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
    "/usr/bin/tailscale",
];

/// Detect this machine's Tailnet IPv4 and cache it on `mesh.tailscale_ip`.
///
/// Always PASSIVE — must never wake or launch Tailscale that is off.
///   1. Interface scan (`find_tailscale_ip`) — zero subprocesses. Hits the
///      common case (daemon running with utun/wintun device) without the CLI.
///   2. Fallback when the scan misses: macOS reads the LocalAPI (the only CLI
///      candidate is the app-bundle shim, which would LAUNCH the GUI app);
///      Linux/Windows run `tailscale ip -4` (no GUI shim, errors if daemon down).
pub fn detect_tailscale(mesh: &Arc<Mutex<MeshManager>>) {
    if let Some(ip) = find_tailscale_ip() {
        log::debug!("[mesh] Tailscale IP detected via interface scan: {}", ip);
        mesh.lock().set_tailscale_ip(Some(ip));
        return;
    }
    #[cfg(target_os = "macos")]
    {
        match super::serve_setup::macos_self_tailscale_ip() {
            Some(ip) => {
                log::debug!("[mesh] Tailscale IP detected via LocalAPI: {}", ip);
                mesh.lock().set_tailscale_ip(Some(ip));
            }
            None => log::debug!("[mesh] Tailscale not available (passive LocalAPI read)"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        for cmd in TAILSCALE_CANDIDATES {
            if let Some(ip) = try_tailscale_probe(cmd) {
                log::debug!("[mesh] Tailscale IP detected via {}: {}", cmd, ip);
                mesh.lock().set_tailscale_ip(Some(ip));
                return;
            }
        }
        log::debug!("[mesh] Tailscale not available");
    }
}

pub fn tailscale_self_dns_name() -> Option<String> {
    let val = tailscale_status_json()?;
    let dns = val
        .get("Self")
        .and_then(|v| v.get("DNSName"))
        .and_then(|v| v.as_str())?
        .trim_end_matches('.')
        .to_string();
    (!dns.is_empty() && dns.ends_with(".ts.net")).then_some(dns)
}

fn tailscale_status_json() -> Option<serde_json::Value> {
    #[cfg(target_os = "macos")]
    {
        super::serve_setup::macos_status_json()
    }
    #[cfg(not(target_os = "macos"))]
    {
        run_tailscale_json(&["status", "--json"], std::time::Duration::from_secs(3))
    }
}

/// Path to the first Tailscale binary that actually responds to `version`, or
/// None when none does. Doubles as an honest "is Tailscale installed" probe:
/// because it runs the binary rather than just stat-ing a path, a stale path or
/// a half-installed GUI bridge reports as not-installed instead of tricking the
/// UI into offering serve setup that can't succeed.
pub(crate) fn tailscale_binary() -> Option<String> {
    for cmd in TAILSCALE_CANDIDATES {
        let mut c = std::process::Command::new(cmd);
        c.arg("version");
        codeterm_util::process_util::no_window_std(&mut c);
        if let Some((true, _)) = run_with_timeout(&mut c, std::time::Duration::from_secs(3)) {
            return Some(cmd.to_string());
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn run_tailscale_json(
    args: &[&str],
    timeout: std::time::Duration,
) -> Option<serde_json::Value> {
    for cmd in TAILSCALE_CANDIDATES {
        let mut c = std::process::Command::new(cmd);
        c.args(args);
        codeterm_util::process_util::no_window_std(&mut c);
        let Some((true, raw)) = run_with_timeout(&mut c, timeout) else {
            continue;
        };
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            return Some(val);
        }
    }
    None
}

/// Scan local network interfaces for an IPv4 address in the Tailscale
/// CGNAT range (100.64.0.0/10). Avoids spawning the `tailscale` CLI,
/// which on Windows briefly flashes a console window every time it runs.
pub fn find_tailscale_ip() -> Option<String> {
    let ifs = if_addrs::get_if_addrs().ok()?;
    for iface in ifs {
        if iface.is_loopback() {
            continue;
        }
        if let if_addrs::IfAddr::V4(v4) = iface.addr {
            let o = v4.ip.octets();
            if o[0] == 100 && (64..=127).contains(&o[1]) {
                return Some(v4.ip.to_string());
            }
        }
    }
    None
}

/// Linux/Windows fallback probe: run `tailscale ip -4`. Safe here — there is no
/// GUI shim to wake, and `tailscale` against a stopped daemon just errors. macOS
/// never reaches this (it reads passively via the LocalAPI in `detect_tailscale`).
#[cfg(not(target_os = "macos"))]
fn try_tailscale_probe(cmd: &str) -> Option<String> {
    let mut command = std::process::Command::new(cmd);
    command.args(["ip", "-4"]);
    codeterm_util::process_util::no_window_std(&mut command);
    let (ok, raw) = run_with_timeout(&mut command, PROBE_TIMEOUT)?;
    if !ok {
        return None;
    }
    parse_first_ip(&raw)
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn parse_first_ip(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let candidate = line.trim();
        candidate
            .parse::<std::net::IpAddr>()
            .ok()
            .map(|_| candidate.to_string())
    })
}

pub(crate) fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
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
            Some(_status) => break,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    log::warn!(
                        "[mesh] tailscale probe timed out after {:?}, killed",
                        timeout
                    );
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    Some((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).to_string(),
    ))
}

pub fn detect_local_ip(mesh: &Arc<Mutex<MeshManager>>) {
    if let Ok(ip) = local_ip_address() {
        mesh.lock().local_ip = Some(ip);
    }
}

fn local_ip_address() -> Result<String, String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket.connect("8.8.8.8:80").map_err(|e| e.to_string())?;
    let addr = socket.local_addr().map_err(|e| e.to_string())?;
    Ok(addr.ip().to_string())
}

pub fn remote_addresses(mesh: &MeshManager) -> Result<Vec<PeerAddress>, String> {
    let addresses = crate::reachability::advertised_addresses(
        &mesh.ready_listeners,
        &mesh.local_announce_addresses(),
        mesh.api_port,
    );
    if addresses.is_empty() {
        Err("mesh_listener_not_remotely_ready".into())
    } else {
        Ok(addresses)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_first_ip;

    #[test]
    fn parse_first_ip_trims_whitespace() {
        for input in ["100.65.0.40\n", "   100.65.0.40   \n"] {
            assert_eq!(parse_first_ip(input).as_deref(), Some("100.65.0.40"));
        }
    }

    #[test]
    fn parse_first_ip_picks_first_of_many() {
        assert_eq!(
            parse_first_ip("100.65.0.40\nfd7a:115c:a1e0::1\n").as_deref(),
            Some("100.65.0.40")
        );
    }

    #[test]
    fn parse_first_ip_rejects_gui_bridge_error_string() {
        let raw = "The Tailscale GUI failed to start: The operation couldn't be completed. (Tailscale.CLIError error 3.)\n";
        assert_eq!(parse_first_ip(raw), None);
    }

    #[test]
    fn parse_first_ip_rejects_empty() {
        assert_eq!(parse_first_ip(""), None);
        assert_eq!(parse_first_ip("\n\n"), None);
    }

    #[test]
    fn parse_first_ip_rejects_non_address_text() {
        assert_eq!(parse_first_ip("not an ip\n"), None);
        assert_eq!(parse_first_ip("100.999.0.1\n"), None);
    }
}
