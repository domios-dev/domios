use std::time::Duration;
use tauri::Emitter;

use crate::MeshStateHandle;

const PING_INTERVAL_SECS: u64 = 20;
const PING_TIMEOUT_SECS: u64 = 4;

pub fn start(mesh: MeshStateHandle, app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        log::info!("[mesh/ping] peer-ping scheduler started (interval {}s)", PING_INTERVAL_SECS);
        loop {
            tick(&mesh, &app_handle).await;
            tokio::time::sleep(Duration::from_secs(PING_INTERVAL_SECS)).await;
        }
    });
}

async fn tick(mesh: &MeshStateHandle, app_handle: &tauri::AppHandle) {
    let targets: Vec<(String, String, u16, bool)> = {
        let m = mesh.lock();
        m.paired_peers.values()
            .filter_map(|p| {
                let addr = p.addresses.iter()
                    .min_by_key(|a| crate::mesh::types::address_tier(a))?;
                Some((p.instance_id.clone(), addr.ip.clone(), p.api_port, p.online))
            })
            .collect()
    };
    if targets.is_empty() { return; }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(PING_TIMEOUT_SECS))
        .build().unwrap_or_default();

    let mut changed = false;
    for (peer_id, ip, port, prev_online) in targets {
        let url = format!("http://{}:{}/api/mesh/ping", ip, port);
        let now_online = matches!(client.get(&url).send().await, Ok(r) if r.status().is_success());
        if now_online != prev_online {
            changed = true;
            let mut m = mesh.lock();
            if let Some(peer) = m.paired_peers.get_mut(&peer_id) {
                peer.online = now_online;
                if now_online {
                    peer.last_seen = Some(chrono::Utc::now().to_rfc3339());
                }
            }
            log::info!("[mesh/ping] peer {} online: {} -> {}", peer_id, prev_online, now_online);
        }
    }
    if changed {
        let _ = app_handle.emit("mesh-state-changed", ());
    }
}
