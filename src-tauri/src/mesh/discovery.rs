//! Peer discovery via mDNS and Tailscale.
//!
//! mDNS: advertises `_codeterm._tcp.local.` and browses for peers.
//! Tailscale: detects Tailscale IP for cross-network connectivity.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use super::types::{DiscoveredPeer, DiscoveryMethod, PeerAddress};
use super::MeshManager;

const SERVICE_TYPE: &str = "_codeterm._tcp.local.";
const PEER_TTL_SECS: u64 = 300; // 5 minutes — mDNS re-announces are infrequent

/// Start mDNS discovery (advertise + browse). Returns a handle to stop it.
pub fn start_mdns(
    mesh: Arc<Mutex<MeshManager>>,
    stealth: bool,
) -> Result<MdnsHandle, String> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mDNS init failed: {}", e))?;

    // Disable IPv6 — mdns_sd prefers IPv6 and skips IPv4 on machines with many bridge interfaces
    if let Err(e) = daemon.disable_interface(mdns_sd::IfKind::IPv6) {
        log::warn!("[mesh/mdns] Failed to disable IPv6: {}", e);
    }

    // Monitor daemon events for diagnostics (interface errors, etc.)
    let daemon_for_monitor = daemon.clone();
    std::thread::spawn(move || {
        if let Ok(monitor) = daemon_for_monitor.monitor() {
            loop {
                match monitor.recv() {
                    Ok(event) => {
                        log::info!("[mesh/mdns] DaemonEvent: {:?}", event);
                    }
                    Err(_) => break,
                }
            }
        }
    });

    // Register our service (unless stealth mode)
    if !stealth {
        let (instance_id, instance_name, api_port) = {
            let m = mesh.lock();
            (
                m.identity.instance_id.clone(),
                m.instance_name.clone(),
                m.api_port,
            )
        };

        let service_name = format!("codeterm-{}", &instance_id[..8]);
        let mut properties = HashMap::new();
        properties.insert("id".to_string(), instance_id);
        properties.insert("name".to_string(), instance_name);
        properties.insert("port".to_string(), api_port.to_string());

        let raw_hostname = hostname::get().unwrap_or_default().to_string_lossy().to_string();
        // Strip existing .local suffix to avoid double .local.local
        let clean_host = raw_hostname.trim_end_matches(".local").trim_end_matches('.');
        let host_fqdn = format!("{}.local.", clean_host);

        // Resolve local IP to bind the service to
        let local_ip = {
            let m = mesh.lock();
            m.local_ip.clone().unwrap_or_default()
        };
        // If we don't have a local IP yet, detect it now
        let bind_ip = if local_ip.is_empty() {
            let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok();
            sock.and_then(|s| { s.connect("8.8.8.8:80").ok()?; s.local_addr().ok() })
                .map(|a| a.ip().to_string())
                .unwrap_or_default()
        } else {
            local_ip
        };

        log::info!("[mesh/mdns] Registering service: type={}, name={}, host={}, ip={}, port={}",
            SERVICE_TYPE, service_name, host_fqdn, bind_ip, api_port);

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

        daemon
            .register(my_service)
            .map_err(|e| format!("mDNS register error: {}", e))?;

        log::info!("[mesh/mdns] Advertising as {}", service_name);
    } else {
        log::info!("[mesh/mdns] Stealth mode — not advertising");
    }

    // Browse for peers
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| format!("mDNS browse error: {}", e))?;

    let mesh_clone = mesh.clone();
    let daemon_clone = daemon.clone();
    let browse_thread = std::thread::spawn(move || {
        let own_id = mesh_clone.lock().identity.instance_id.clone();

        loop {
            match receiver.recv() {
                Ok(event) => match event {
                    mdns_sd::ServiceEvent::ServiceResolved(info) => {
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
                        let port_str = info
                            .get_property_val_str("port")
                            .unwrap_or_default()
                            .to_string();
                        let port: u16 = port_str.parse().unwrap_or(info.get_port());

                        let addresses: Vec<PeerAddress> = info
                            .get_addresses()
                            .iter()
                            .map(|addr| PeerAddress {
                                ip: addr.to_string(),
                                is_tailscale: false,
                            })
                            .collect();

                        if addresses.is_empty() {
                            continue;
                        }

                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        let peer = DiscoveredPeer {
                            instance_id: id.clone(),
                            instance_name: name.clone(),
                            addresses,
                            api_port: port,
                            discovery_method: DiscoveryMethod::Mdns,
                            last_seen: now,
                            dev: false,
                            mesh_id: None,
                        };

                        log::debug!("[mesh/mdns] Discovered: {} ({})", name, id);
                        mesh_clone.lock().discovered_peers.insert(id, peer);
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, full_name) => {
                        log::debug!("[mesh/mdns] Service removed: {}", full_name);
                        // Will be cleaned up by TTL
                    }
                    mdns_sd::ServiceEvent::SearchStarted(msg) => {
                        log::info!("[mesh/mdns] Browse started: {}", msg);
                    }
                    mdns_sd::ServiceEvent::ServiceFound(ty, name) => {
                        log::info!("[mesh/mdns] ServiceFound: type={}, name={}", ty, name);
                    }
                    mdns_sd::ServiceEvent::SearchStopped(ty) => {
                        log::info!("[mesh/mdns] Browse stopped: {}", ty);
                    }
                },
                Err(e) => {
                    log::warn!("[mesh/mdns] Browse receiver error: {}", e);
                    break;
                }
            }
        }

        let _ = daemon_clone.shutdown();
    });

    mesh.lock().discovery_active = true;

    Ok(MdnsHandle {
        daemon,
        _browse_thread: browse_thread,
    })
}

/// Handle to stop mDNS discovery.
pub struct MdnsHandle {
    daemon: mdns_sd::ServiceDaemon,
    _browse_thread: std::thread::JoinHandle<()>,
}

impl MdnsHandle {
    pub fn shutdown(self) {
        let _ = self.daemon.shutdown();
    }
}

/// Detect Tailscale IP and store it in mesh state.
/// Checks multiple paths: `tailscale` in PATH (Linux/Windows),
/// macOS app bundle, and common install locations.
pub fn detect_tailscale(mesh: &Arc<Mutex<MeshManager>>) {
    let candidates = [
        "tailscale".to_string(),
        // macOS app bundle
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale".to_string(),
        // Linux common paths
        "/usr/bin/tailscale".to_string(),
        "/usr/local/bin/tailscale".to_string(),
    ];

    for cmd in &candidates {
        if let Ok(output) = std::process::Command::new(cmd).args(["ip", "-4"]).output() {
            if output.status.success() {
                let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !ip.is_empty() {
                    log::info!("[mesh] Tailscale IP detected via {}: {}", cmd, ip);
                    mesh.lock().tailscale_ip = Some(ip);
                    return;
                }
            }
        }
    }
    log::debug!("[mesh] Tailscale not available");
}

/// Detect local IP address.
pub fn detect_local_ip(mesh: &Arc<Mutex<MeshManager>>) {
    if let Ok(ip) = local_ip_address() {
        mesh.lock().local_ip = Some(ip);
    }
}

fn local_ip_address() -> Result<String, String> {
    // Bind a UDP socket to a public address to discover the local IP
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    socket
        .connect("8.8.8.8:80")
        .map_err(|e| e.to_string())?;
    let addr = socket.local_addr().map_err(|e| e.to_string())?;
    Ok(addr.ip().to_string())
}

/// Remove discovered peers that haven't been seen within TTL.
pub fn cleanup_stale_peers(mesh: &Arc<Mutex<MeshManager>>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    mesh.lock()
        .discovered_peers
        .retain(|_, p| now.saturating_sub(p.last_seen) < PEER_TTL_SECS);
}

// ── UDP Broadcast fallback ──────────────────────────────────────────

// No separate broadcast port — we use the API port (TCP/UDP don't conflict)
const BROADCAST_INTERVAL_SECS: u64 = 10;
const BROADCAST_MAGIC: &[u8] = b"CODETERM1";

/// Beacon payload: magic + JSON.
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

/// Start UDP broadcast discovery (fallback when mDNS fails).
/// Sends beacon every 10s to 255.255.255.255:7687 and listens for others.
pub fn start_udp_broadcast(mesh: Arc<Mutex<MeshManager>>) -> Result<(), String> {
    let (instance_id, instance_name, api_port) = {
        let m = mesh.lock();
        (
            m.identity.instance_id.clone(),
            m.instance_name.clone(),
            m.api_port,
        )
    };

    // Sender thread — rebuilds beacon each cycle to pick up mesh_id changes
    let mesh_for_sender = mesh.clone();
    let sender_instance_id = instance_id.clone();
    let sender_instance_name = instance_name.clone();
    std::thread::spawn(move || {
        let instance_id = sender_instance_id;
        let instance_name = sender_instance_name;
        let sock = match std::net::UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => s,
            Err(e) => {
                log::error!("[mesh/udp] Failed to bind sender: {}", e);
                return;
            }
        };
        let _ = sock.set_broadcast(true);
        log::info!("[mesh/udp] Broadcast sender started, target port {}", api_port);
        loop {
            // Rebuild beacon each cycle to pick up mesh_id changes
            let mesh_id = {
                let m = mesh_for_sender.lock();
                if m.paired_peers.is_empty() {
                    None // Not in a mesh
                } else {
                    // Extract mesh_id from crown JWT, or use a hash of crown holder as fallback
                    m.crown_public.as_ref().and_then(|c| {
                        crate::mesh::crown::verify_crown_jwt(&c.crown_jwt, &c.public_key)
                            .ok().map(|cl| cl.mesh_id)
                    }).or_else(|| {
                        // Fallback: use crown holder_id as mesh identifier
                        m.crown_public.as_ref().map(|c| c.holder_id.clone())
                    })
                }
            };
            let beacon = Beacon {
                id: instance_id.clone(),
                name: instance_name.clone(),
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
            std::thread::sleep(std::time::Duration::from_secs(BROADCAST_INTERVAL_SECS));
        }
    });

    // Listener thread
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
        log::info!("[mesh/udp] Broadcast listener started on port {}", api_port);

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
                    let json_data = &buf[BROADCAST_MAGIC.len()..len];
                    let beacon: Beacon = match serde_json::from_slice(json_data) {
                        Ok(b) => b,
                        Err(_) => continue,
                    };
                    // Skip our own broadcasts
                    if beacon.id == instance_id {
                        continue;
                    }

                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();

                    let peer = DiscoveredPeer {
                        instance_id: beacon.id.clone(),
                        instance_name: beacon.name,
                        addresses: vec![PeerAddress {
                            ip: src_addr.ip().to_string(),
                            is_tailscale: false,
                        }],
                        api_port: beacon.port,
                        discovery_method: DiscoveryMethod::Udp,
                        last_seen: now,
                        dev: beacon.dev,
                        mesh_id: beacon.mesh_id.clone(),
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

    log::info!("[mesh/udp] UDP broadcast discovery started");
    Ok(())
}
