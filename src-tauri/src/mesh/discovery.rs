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
const PEER_TTL_SECS: u64 = 30;

/// Start mDNS discovery (advertise + browse). Returns a handle to stop it.
pub fn start_mdns(
    mesh: Arc<Mutex<MeshManager>>,
    stealth: bool,
) -> Result<MdnsHandle, String> {
    let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mDNS init failed: {}", e))?;

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

        let my_service = mdns_sd::ServiceInfo::new(
            SERVICE_TYPE,
            &service_name,
            &format!("{}.local.", hostname::get().unwrap_or_default().to_string_lossy()),
            "",
            api_port,
            properties,
        )
        .map_err(|e| format!("mDNS service info error: {}", e))?;

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
                        };

                        log::debug!("[mesh/mdns] Discovered: {} ({})", name, id);
                        mesh_clone.lock().discovered_peers.insert(id, peer);
                    }
                    mdns_sd::ServiceEvent::ServiceRemoved(_, full_name) => {
                        log::debug!("[mesh/mdns] Service removed: {}", full_name);
                        // Will be cleaned up by TTL
                    }
                    _ => {}
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
pub fn detect_tailscale(mesh: &Arc<Mutex<MeshManager>>) {
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !ip.is_empty() {
                log::info!("[mesh] Tailscale IP detected: {}", ip);
                mesh.lock().tailscale_ip = Some(ip);
            }
        }
        _ => {
            log::debug!("[mesh] Tailscale not available");
        }
    }
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
