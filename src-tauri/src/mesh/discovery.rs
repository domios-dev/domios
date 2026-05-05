use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::types::{DiscoveredPeer, DiscoveryMethod, PeerAddress};
use super::MeshManager;

const SERVICE_TYPE: &str = "_codeterm._tcp.local.";
const PEER_TTL_SECS: u64 = 300;
/// How often the background loop actively pulls gossip from each paired
/// peer to refresh discovered_peers' last_seen. Must stay strictly below
/// PEER_TTL_SECS so that discovered peers known only via gossip aren't
/// pruned between pushes. 60s gives ~5x safety margin against the 300s
/// TTL and matches the existing 15s health-loop cadence (every 4th tick).
pub const PROACTIVE_SYNC_INTERVAL_SECS: u64 = 60;
const BROADCAST_INTERVAL_SECS: u64 = 30;
const BROADCAST_MAGIC: &[u8] = b"CODETERM1";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── mDNS ───────────────────────────────────────────────────────────────

/// Initialize mDNS daemon and start browsing for peers.
/// Does NOT register our service — call `register_mdns_service` separately.
/// Idempotent: skips if daemon already exists.
pub fn init_mdns_daemon(mesh: Arc<Mutex<MeshManager>>) -> Result<(), String> {
    if mesh.lock().mdns_daemon.is_some() {
        return Ok(());
    }

    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mDNS init failed: {}", e))?;
    if let Err(e) = daemon.disable_interface(mdns_sd::IfKind::IPv6) {
        log::warn!("[mesh/mdns] Failed to disable IPv6: {}", e);
    }

    let daemon_for_monitor = daemon.clone();
    std::thread::spawn(move || {
        if let Ok(monitor) = daemon_for_monitor.monitor() {
            loop {
                match monitor.recv() {
                    Ok(event) => log::info!("[mesh/mdns] DaemonEvent: {:?}", event),
                    Err(_) => break,
                }
            }
        }
    });

    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| format!("mDNS browse error: {}", e))?;

    let mesh_clone = mesh.clone();
    std::thread::spawn(move || {
        let own_id = mesh_clone.lock().ident.identity.instance_id.clone();
        loop {
            match receiver.recv() {
                Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                    let id = info
                        .get_property_val_str("id")
                        .unwrap_or_default()
                        .to_string();
                    if id.is_empty() || id == own_id {
                        continue;
                    }
                    let name = info
                        .get_property_val_str("name")
                        .unwrap_or_default()
                        .to_string();
                    let port: u16 = info
                        .get_property_val_str("port")
                        .unwrap_or_default()
                        .parse()
                        .unwrap_or(info.get_port());
                    let addresses: Vec<PeerAddress> = info
                        .get_addresses()
                        .iter()
                        .map(|addr| PeerAddress::from_ip(&addr.to_string()))
                        .collect();
                    if addresses.is_empty() {
                        continue;
                    }
                    let peer = DiscoveredPeer {
                        instance_id: id.clone(),
                        instance_name: name.clone(),
                        addresses,
                        api_port: port,
                        discovery_method: DiscoveryMethod::Mdns,
                        last_seen: now_secs(),
                        dev: false,
                        mesh_id: None,
                        vouched_by: Vec::new(),
                    };
                    log::debug!("[mesh/mdns] Discovered: {} ({})", name, id);
                    mesh_clone.lock().discovered_peers.insert(id, peer);
                }
                Ok(mdns_sd::ServiceEvent::SearchStarted(msg)) => {
                    log::info!("[mesh/mdns] Browse started: {}", msg);
                }
                Ok(mdns_sd::ServiceEvent::ServiceFound(ty, name)) => {
                    log::info!("[mesh/mdns] ServiceFound: type={}, name={}", ty, name);
                }
                Ok(mdns_sd::ServiceEvent::ServiceRemoved(_, full_name)) => {
                    log::debug!("[mesh/mdns] Service removed: {}", full_name);
                }
                Ok(mdns_sd::ServiceEvent::SearchStopped(ty)) => {
                    log::info!("[mesh/mdns] Browse stopped: {}", ty);
                }
                Err(e) => {
                    log::warn!("[mesh/mdns] Browse receiver error: {}", e);
                    break;
                }
            }
        }
    });

    mesh.lock().mdns_daemon = Some(daemon);
    mesh.lock().discovery_active = true;
    log::info!("[mesh/mdns] Daemon initialized, browsing for peers");
    Ok(())
}

/// Register our mDNS service so other peers can discover us.
pub fn register_mdns_service(mesh: &Arc<Mutex<MeshManager>>) -> Result<(), String> {
    let m = mesh.lock();
    let daemon = m.mdns_daemon.as_ref().ok_or("mDNS daemon not initialized")?;
    let daemon = daemon.clone();

    let instance_id = m.ident.identity.instance_id.clone();
    let instance_name = m.ident.instance_name.clone();
    let api_port = m.api_port;
    let local_ip = m.local_ip.clone().unwrap_or_default();
    drop(m);

    let bind_ip = if local_ip.is_empty() {
        let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok();
        sock.and_then(|s| {
            s.connect("8.8.8.8:80").ok()?;
            s.local_addr().ok()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_default()
    } else {
        local_ip
    };

    let service_name = format!("codeterm-{}", &instance_id[..8]);
    let mut properties = HashMap::new();
    properties.insert("id".to_string(), instance_id);
    properties.insert("name".to_string(), instance_name);
    properties.insert("port".to_string(), api_port.to_string());

    let raw_hostname = hostname::get()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let clean_host = raw_hostname
        .trim_end_matches(".local")
        .trim_end_matches('.');
    let host_fqdn = format!("{}.local.", clean_host);

    log::info!(
        "[mesh/mdns] Registering service: name={}, host={}, ip={}, port={}",
        service_name, host_fqdn, bind_ip, api_port
    );

    let my_service = mdns_sd::ServiceInfo::new(
        SERVICE_TYPE,
        &service_name,
        &host_fqdn,
        bind_ip.as_str(),
        api_port,
        properties,
    )
    .map_err(|e| format!("mDNS service info error: {}", e))?
    .enable_addr_auto();

    let fullname = my_service.get_fullname().to_string();
    daemon
        .register(my_service)
        .map_err(|e| format!("mDNS register error: {}", e))?;

    mesh.lock().mdns_service_fullname = Some(fullname.clone());
    log::info!("[mesh/mdns] Advertising as {}", fullname);
    Ok(())
}

/// Unregister our mDNS service (stop advertising).
pub fn unregister_mdns_service(mesh: &Arc<Mutex<MeshManager>>) {
    let m = mesh.lock();
    let (daemon, fullname) = match (m.mdns_daemon.as_ref(), m.mdns_service_fullname.as_ref()) {
        (Some(d), Some(f)) => (d.clone(), f.clone()),
        _ => return,
    };
    drop(m);

    if let Err(e) = daemon.unregister(&fullname) {
        log::warn!("[mesh/mdns] Unregister failed: {}", e);
    } else {
        log::info!("[mesh/mdns] Unregistered service {}", fullname);
    }
    mesh.lock().mdns_service_fullname = None;
}

// ── Tailscale / Local IP ───────────────────────────────────────────────

/// Hard cap on how long any single Tailscale CLI probe is allowed to run.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

enum ProbeOutcome {
    Found(String),
    NoBinary,
    GuiBridgeError,
}

/// Detect this machine's Tailnet IPv4 and cache it on `mesh.tailscale_ip`.
///
/// Two-stage strategy:
///   1. Interface scan (`find_tailscale_ip`) — zero subprocesses, no console
///      flash on Windows. Hits the common case (Tailscale daemon running with
///      utun/wintun device) without ever spawning the CLI.
///   2. CLI cascade fallback — only runs if the scan misses. Tries common
///      binary paths and on macOS retries via `launchctl asuser <uid>` for
///      GUI-bridge installs that need to attach to the user's launchd
///      bootstrap to resolve XPC. All spawns go through `no_window_std` so
///      the fallback path also stays console-flash free on Windows.
///
/// Each probe is bounded by `PROBE_TIMEOUT` (5s).
pub fn detect_tailscale(mesh: &Arc<Mutex<MeshManager>>) {
    if let Some(ip) = find_tailscale_ip() {
        log::info!("[mesh] Tailscale IP detected via interface scan: {}", ip);
        mesh.lock().tailscale_ip = Some(ip);
        return;
    }
    let candidates = [
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        "/opt/homebrew/bin/tailscale",
        "/usr/local/bin/tailscale",
        "/usr/bin/tailscale",
    ];
    for cmd in &candidates {
        match try_tailscale_probe(cmd, ProbeMode::Direct) {
            ProbeOutcome::Found(ip) => {
                log::info!("[mesh] Tailscale IP detected via {}: {}", cmd, ip);
                mesh.lock().tailscale_ip = Some(ip);
                return;
            }
            ProbeOutcome::GuiBridgeError => {
                #[cfg(target_os = "macos")]
                {
                    if let ProbeOutcome::Found(ip) =
                        try_tailscale_probe(cmd, ProbeMode::LaunchctlAsuser)
                    {
                        log::info!(
                            "[mesh] Tailscale IP detected via launchctl asuser {}: {}",
                            cmd, ip
                        );
                        mesh.lock().tailscale_ip = Some(ip);
                        return;
                    }
                }
            }
            ProbeOutcome::NoBinary => {}
        }
    }
    log::debug!("[mesh] Tailscale not available");
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

enum ProbeMode {
    Direct,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    LaunchctlAsuser,
}

fn try_tailscale_probe(cmd: &str, mode: ProbeMode) -> ProbeOutcome {
    let mut command = match mode {
        ProbeMode::Direct => {
            let mut c = std::process::Command::new(cmd);
            c.args(["ip", "-4"]);
            c
        }
        ProbeMode::LaunchctlAsuser => {
            #[cfg(target_os = "macos")]
            {
                let uid = match current_uid() {
                    Some(u) if u != "0" => u,
                    Some(_) => {
                        log::debug!(
                            "[mesh] skipping launchctl asuser fallback for {}: running as uid 0",
                            cmd
                        );
                        return ProbeOutcome::NoBinary;
                    }
                    None => {
                        log::warn!(
                            "[mesh] skipping launchctl asuser fallback for {}: cannot resolve current uid",
                            cmd
                        );
                        return ProbeOutcome::NoBinary;
                    }
                };
                let mut c = std::process::Command::new("/bin/launchctl");
                c.args(["asuser", &uid, cmd, "ip", "-4"]);
                c
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = cmd;
                return ProbeOutcome::NoBinary;
            }
        }
    };
    crate::process_util::no_window_std(&mut command);
    let raw = match run_with_timeout(&mut command, PROBE_TIMEOUT) {
        Some((true, raw)) => raw,
        _ => return ProbeOutcome::NoBinary,
    };
    if let Some(ip) = parse_first_ip(&raw) {
        return ProbeOutcome::Found(ip);
    }
    if raw.trim().is_empty() {
        return ProbeOutcome::NoBinary;
    }
    if matches!(mode, ProbeMode::Direct) {
        log::warn!(
            "[mesh] {} ip -4 returned non-IP output (likely Tailscale GUI bridge error): {}",
            cmd,
            raw.lines().next().unwrap_or("").chars().take(120).collect::<String>()
        );
    } else {
        log::debug!(
            "[mesh] launchctl asuser {} ip -4 still returned non-IP output: {}",
            cmd,
            raw.lines().next().unwrap_or("").chars().take(120).collect::<String>()
        );
    }
    ProbeOutcome::GuiBridgeError
}

fn parse_first_ip(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let candidate = line.trim();
        candidate
            .parse::<std::net::IpAddr>()
            .ok()
            .map(|_| candidate.to_string())
    })
}

fn run_with_timeout(
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

#[cfg(target_os = "macos")]
fn current_uid() -> Option<String> {
    let mut cmd = std::process::Command::new("/usr/bin/id");
    cmd.arg("-u");
    crate::process_util::no_window_std(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.parse::<u32>().is_ok() { Some(s) } else { None }
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

// ── Stale peer cleanup ─────────────────────────────────────────────────

pub fn cleanup_stale_peers(mesh: &Arc<Mutex<MeshManager>>) {
    let now = now_secs();
    mesh.lock()
        .discovered_peers
        .retain(|_, p| now.saturating_sub(p.last_seen) < PEER_TTL_SECS);
}

// ── UDP Broadcast ──────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
struct Beacon {
    id: String,
    name: String,
    port: u16,
    #[serde(default)]
    dev: bool,
    #[serde(default)]
    mesh_id: Option<String>,
}

/// Start UDP broadcast threads (sender + listener). Idempotent.
/// Sender only sends when `broadcast_active` is true in MeshManager.
pub fn start_udp_threads(mesh: Arc<Mutex<MeshManager>>) -> Result<(), String> {
    if mesh.lock().udp_started {
        return Ok(());
    }
    mesh.lock().udp_started = true;

    let (instance_id, instance_name, api_port) = {
        let m = mesh.lock();
        (
            m.ident.identity.instance_id.clone(),
            m.ident.instance_name.clone(),
            m.api_port,
        )
    };

    // Sender — only sends when broadcast_active
    let mesh_for_sender = mesh.clone();
    let sender_id = instance_id.clone();
    let sender_name = instance_name.clone();
    std::thread::spawn(move || {
        let sock = match std::net::UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                log::error!("[mesh/udp] Failed to bind sender: {}", e);
                return;
            }
        };
        let _ = sock.set_broadcast(true);
        log::info!("[mesh/udp] Sender thread started, target port {}", api_port);
        loop {
            let active = mesh_for_sender.lock().broadcast_active;
            if active {
                let mesh_id = {
                    let m = mesh_for_sender.lock();
                    if m.paired_peers.is_empty() {
                        None
                    } else {
                        Some(m.ident.identity.instance_id.clone())
                    }
                };
                let beacon = Beacon {
                    id: sender_id.clone(),
                    name: sender_name.clone(),
                    port: api_port,
                    dev: cfg!(debug_assertions),
                    mesh_id,
                };
                if let Ok(json) = serde_json::to_vec(&beacon) {
                    let mut packet = Vec::with_capacity(BROADCAST_MAGIC.len() + json.len());
                    packet.extend_from_slice(BROADCAST_MAGIC);
                    packet.extend_from_slice(&json);
                    if let Err(e) = sock.send_to(&packet, format!("255.255.255.255:{}", api_port)) {
                        log::debug!("[mesh/udp] Send failed: {}", e);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(BROADCAST_INTERVAL_SECS));
        }
    });

    // Listener — always active
    let mesh_clone = mesh.clone();
    std::thread::spawn(move || {
        let sock = match std::net::UdpSocket::bind(format!("0.0.0.0:{}", api_port)) {
            Ok(s) => s,
            Err(e) => {
                log::error!("[mesh/udp] Failed to bind listener on port {}: {}", api_port, e);
                return;
            }
        };
        let _ = sock.set_broadcast(true);
        log::info!("[mesh/udp] Listener started on port {}", api_port);

        let mut buf = [0u8; 1024];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((len, src_addr)) => {
                    if len <= BROADCAST_MAGIC.len() {
                        continue;
                    }
                    if &buf[..BROADCAST_MAGIC.len()] != BROADCAST_MAGIC {
                        continue;
                    }
                    let beacon: Beacon = match serde_json::from_slice(&buf[BROADCAST_MAGIC.len()..len]) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    if beacon.id == instance_id {
                        continue;
                    }
                    let peer = DiscoveredPeer {
                        instance_id: beacon.id.clone(),
                        instance_name: beacon.name,
                        addresses: vec![PeerAddress::from_ip(&src_addr.ip().to_string())],
                        api_port: beacon.port,
                        discovery_method: DiscoveryMethod::Udp,
                        last_seen: now_secs(),
                        dev: beacon.dev,
                        mesh_id: beacon.mesh_id.clone(),
                        vouched_by: Vec::new(),
                    };
                    let mut m = mesh_clone.lock();
                    let is_new = !m.discovered_peers.contains_key(&beacon.id);
                    m.discovered_peers.insert(beacon.id.clone(), peer);
                    drop(m);
                    if is_new {
                        log::info!(
                            "[mesh/udp] Discovered peer: {} ({}) at {}",
                            beacon.id, src_addr.ip(), beacon.port
                        );
                    }
                }
                Err(e) => {
                    log::debug!("[mesh/udp] Recv error: {}", e);
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
    });

    log::info!("[mesh/udp] UDP threads started");
    Ok(())
}

// ── Orchestration ──────────────────────────────────────────────────────

/// Start listeners only (mDNS browse + UDP receiver). No broadcasting.
/// Call on app startup when mesh is enabled.
pub fn start_listeners(mesh: Arc<Mutex<MeshManager>>) -> Result<(), String> {
    if let Err(e) = init_mdns_daemon(mesh.clone()) {
        log::error!("[mesh] mDNS daemon init failed: {}", e);
    }
    if let Err(e) = start_udp_threads(mesh) {
        log::error!("[mesh] UDP threads start failed: {}", e);
    }
    Ok(())
}

/// Start broadcasting (mDNS + UDP) for `duration_secs`.
/// Listeners are started if not already running.
/// After the duration, broadcasting stops automatically.
pub fn start_broadcast(
    mesh: Arc<Mutex<MeshManager>>,
    duration_secs: u64,
) -> Result<(), String> {
    let expires_at = now_secs() + duration_secs;
    {
        let mut m = mesh.lock();
        m.broadcast_active = true;
        m.broadcast_expires_at = Some(expires_at);
    }

    if let Err(e) = init_mdns_daemon(mesh.clone()) {
        log::error!("[mesh] mDNS daemon init failed: {}", e);
    }
    if let Err(e) = register_mdns_service(&mesh) {
        log::error!("[mesh] mDNS register failed: {}", e);
    }
    if let Err(e) = start_udp_threads(mesh.clone()) {
        log::error!("[mesh] UDP start failed: {}", e);
    }

    let mesh_for_timer = mesh.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(duration_secs));
        let mut m = mesh_for_timer.lock();
        if m.broadcast_expires_at == Some(expires_at) {
            m.broadcast_active = false;
            m.broadcast_expires_at = None;
            drop(m);
            unregister_mdns_service(&mesh_for_timer);
            log::info!("[mesh] Broadcast expired after {}s", duration_secs);
        }
    });

    log::info!(
        "[mesh] Broadcasting started for {}s (expires at {})",
        duration_secs, expires_at
    );
    Ok(())
}

/// Stop broadcasting immediately.
pub fn stop_broadcast(mesh: &Arc<Mutex<MeshManager>>) {
    {
        let mut m = mesh.lock();
        if !m.broadcast_active {
            return;
        }
        m.broadcast_active = false;
        m.broadcast_expires_at = None;
    }
    unregister_mdns_service(mesh);
    log::info!("[mesh] Broadcast stopped manually");
}

#[cfg(test)]
mod tests {
    use super::parse_first_ip;

    #[test]
    fn parse_first_ip_happy_path() {
        assert_eq!(parse_first_ip("100.64.10.3\n").as_deref(), Some("100.64.10.3"));
    }

    #[test]
    fn parse_first_ip_trims_whitespace() {
        assert_eq!(
            parse_first_ip("   100.64.10.3   \n").as_deref(),
            Some("100.64.10.3")
        );
    }

    #[test]
    fn parse_first_ip_picks_first_of_many() {
        assert_eq!(
            parse_first_ip("100.64.10.3\nfd7a:115c:a1e0::1\n").as_deref(),
            Some("100.64.10.3")
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
