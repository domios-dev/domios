use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;

use crate::config::ConfigState;
use crate::MeshStateHandle;

const PING_INTERVAL_SECS: u64 = 20;
const PING_TIMEOUT_SECS: u64 = 4;
const MISS_THRESHOLD: u8 = 2;

pub fn start(mesh: MeshStateHandle, config: ConfigState, app_handle: tauri::AppHandle) {
    let client = Arc::new(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(PING_TIMEOUT_SECS))
            .pool_idle_timeout(Some(Duration::from_secs(120)))
            .build()
            .unwrap_or_default(),
    );
    tauri::async_runtime::spawn(async move {
        log::info!(
            "[mesh/ping] peer-ping scheduler started (interval {}s)",
            PING_INTERVAL_SECS
        );
        loop {
            tick(&mesh, &config, &app_handle, &client).await;
            tokio::time::sleep(Duration::from_secs(PING_INTERVAL_SECS)).await;
        }
    });
}

async fn tick(
    mesh: &MeshStateHandle,
    config: &ConfigState,
    app_handle: &tauri::AppHandle,
    client: &reqwest::Client,
) {
    if !config.lock().mesh.enabled {
        return;
    }
    let targets: Vec<(String, String, u16, bool, u8)> = {
        let m = mesh.lock();
        m.paired_peers
            .values()
            .filter_map(|p| {
                let addr = p
                    .addresses
                    .iter()
                    .min_by_key(|a| crate::mesh::types::address_tier(a))?;
                Some((
                    p.instance_id.clone(),
                    addr.ip.clone(),
                    p.api_port,
                    p.online,
                    p.consecutive_misses,
                ))
            })
            .collect()
    };
    if targets.is_empty() {
        return;
    }

    let mut changed = false;
    for (peer_id, ip, port, prev_online, misses) in targets {
        let url = format!("http://{}:{}/api/mesh/ping", ip, port);
        let now_online = matches!(client.get(&url).send().await, Ok(r) if r.status().is_success());

        let mut m = mesh.lock();
        if let Some(peer) = m.paired_peers.get_mut(&peer_id) {
            if now_online {
                peer.consecutive_misses = 0;
                peer.last_seen = Some(chrono::Utc::now().to_rfc3339());
                if !prev_online {
                    peer.online = true;
                    changed = true;
                    log::info!("[mesh/ping] peer {} online: false -> true", peer_id);
                }
            } else {
                let new_misses = misses.saturating_add(1);
                peer.consecutive_misses = new_misses;
                if peer_should_flip_offline(prev_online, new_misses) {
                    peer.online = false;
                    changed = true;
                    log::info!(
                        "[mesh/ping] peer {} online: true -> false (miss {})",
                        peer_id,
                        new_misses
                    );
                }
            }
        }
    }
    if changed {
        let _ = app_handle.emit("mesh-state-changed", ());
    }
}

/// Returns true when a peer that was online has now accumulated enough
/// consecutive misses to be declared offline.
pub fn peer_should_flip_offline(prev_online: bool, consecutive_misses: u8) -> bool {
    prev_online && consecutive_misses >= MISS_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_miss_does_not_flip_online_peer_offline() {
        assert!(!peer_should_flip_offline(true, 1));
    }

    #[test]
    fn two_misses_flips_online_peer_offline() {
        assert!(peer_should_flip_offline(true, MISS_THRESHOLD));
    }

    #[test]
    fn already_offline_peer_is_never_flipped_again() {
        assert!(!peer_should_flip_offline(false, MISS_THRESHOLD));
        assert!(!peer_should_flip_offline(false, 5));
    }

    #[test]
    fn above_threshold_also_flips() {
        assert!(peer_should_flip_offline(true, MISS_THRESHOLD + 1));
        assert!(peer_should_flip_offline(true, u8::MAX));
    }

    #[test]
    fn zero_misses_never_flips() {
        assert!(!peer_should_flip_offline(true, 0));
        assert!(!peer_should_flip_offline(false, 0));
    }
}
