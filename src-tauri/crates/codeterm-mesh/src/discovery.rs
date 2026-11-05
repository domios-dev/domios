//! Self endpoint detection only. New peers come from the account registry, never from
//! LAN beacons, mDNS browsing or Tailnet-wide enumeration.

use std::sync::Arc;

use parking_lot::Mutex;

use super::types::PeerAddress;
use super::MeshManager;

/// Cadence of authenticated sync with already-paired peers.
pub const PROACTIVE_SYNC_INTERVAL_SECS: u64 = 60;

// ── Tailscale / Local IP ───────────────────────────────────────────────

/// Binary lookup order for the Tailscale CLI: bare name (resolved via PATH)
/// first, then the common absolute install locations (macOS app bundle,
/// Homebrew, Linux package). Shared by every probe + the serve-setup module.
pub(crate) const TAILSCALE_CANDIDATES: [&str; 6] = [
    "tailscale",
    "C:/Program Files/Tailscale/tailscale.exe",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
    "/usr/bin/tailscale",
];

pub fn detect_tailscale(mesh: &Arc<Mutex<MeshManager>>) {
    let status = crate::network_transport::active_provider().detect();
    mesh.lock().set_network_transport(status);
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

pub(crate) fn tailscale_status_json() -> Option<serde_json::Value> {
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
        if let Some((true, _)) = run_with_timeout(c, std::time::Duration::from_secs(3)) {
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
        let Some((success, raw)) = run_with_timeout(c, timeout) else {
            continue;
        };
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            if success || (args == ["status", "--json"] && val["BackendState"].is_string()) {
                return Some(val);
            }
        }
    }
    None
}

/// Scan local network interfaces for an IPv4 address in the Tailscale
/// CGNAT range (100.64.0.0/10). Avoids spawning the `tailscale` CLI,
/// which on Windows briefly flashes a console window every time it runs.
pub fn find_tailscale_ip() -> Option<String> {
    let ifs = if_addrs::get_if_addrs().ok()?;
    let mut addresses: Vec<_> = ifs
        .into_iter()
        .filter(|iface| !iface.is_loopback())
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(v4) if crate::types::is_tailscale_ip(&v4.ip.to_string()) => {
                Some(v4.ip)
            }
            _ => None,
        })
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    addresses.first().map(ToString::to_string)
}

#[cfg(test)]
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
    cmd: std::process::Command,
    timeout: std::time::Duration,
) -> Option<(bool, String)> {
    let output = match codeterm_util::process_util::bounded_output(cmd, timeout) {
        Ok(output) => output,
        Err(error) => {
            log::debug!("[mesh] tailscale probe failed: {error}");
            return None;
        }
    };
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

pub(crate) fn local_ip_address() -> Result<String, String> {
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
mod tests;
