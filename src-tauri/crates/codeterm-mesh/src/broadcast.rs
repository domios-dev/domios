use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};

use parking_lot::Mutex;

use super::MeshState;
use super::{discovery, transport};

pub async fn broadcast_sync(mesh: &MeshState) {
    let (payload, clients) = {
        let mut m = mesh.lock();
        let payload = m.build_sync_payload();
        let clients: Vec<_> = m
            .paired_peers
            .values()
            .filter_map(|p| {
                if p.addresses.is_empty() {
                    return None;
                }
                let token = m.sign_request_for(&p.instance_id).ok()?;
                Some((
                    p.addresses.clone(),
                    p.api_port,
                    token,
                    p.instance_id.clone(),
                ))
            })
            .collect();
        (payload, clients)
    };

    if clients.is_empty() {
        return;
    }
    let http = transport::shared_http_client();
    for (addresses, port, token, peer_id) in &clients {
        let result = transport::try_peer(addresses, |addr| {
            let url = format!("http://{}:{}/api/mesh/sync", addr.ip, port);
            let http = http.clone();
            let token = token.clone();
            let payload = payload.clone();
            async move {
                match http
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => transport::TryNext::Success(()),
                    Ok(resp) => {
                        let status = resp.status();
                        transport::TryNext::Stop(format!("HTTP {}", status))
                    }
                    Err(_) => transport::TryNext::Retry("request failed".to_string()),
                }
            }
        })
        .await;
        match result {
            Ok(()) => log::debug!("[mesh/broadcast] Synced to {}", peer_id),
            Err(e) => log::warn!(
                "[mesh/broadcast] Sync failed: {}",
                peer_request_error(peer_id, &e)
            ),
        }
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AnnounceReport {
    pub sent: usize,
    pub ok: usize,
    pub stale_peers: Vec<String>,
    pub errors: Vec<String>,
}

fn peer_request_error(peer_id: &str, _detail: &str) -> String {
    format!("{peer_id}: request failed")
}

fn announce_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn refresh_gate() -> &'static std::sync::Mutex<()> {
    static GATE: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| std::sync::Mutex::new(()))
}

fn refresh_generation() -> &'static AtomicU64 {
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    &GENERATION
}

pub async fn broadcast_announce(mesh: &MeshState, target: Option<&str>) -> AnnounceReport {
    let _publication_guard = announce_gate().lock().await;
    let mut report = AnnounceReport::default();
    let (announce, membership, clients) = {
        let m = mesh.lock();
        let announce = m.build_announce();
        let membership = m.advertised_membership();
        let token = match m.sign_request() {
            Ok(t) => t,
            Err(e) => {
                report.errors.push(format!("sign: {}", e));
                return report;
            }
        };
        let clients: Vec<_> = m
            .paired_peers
            .values()
            .filter(|p| {
                target
                    .map(|t| t == p.instance_id.as_str() || t == p.instance_name.as_str())
                    .unwrap_or(true)
            })
            .filter(|p| m.announce_allowed(&p.instance_id))
            .filter_map(|p| {
                if p.addresses.is_empty() {
                    return None;
                }
                Some((
                    p.addresses.clone(),
                    p.api_port,
                    token.clone(),
                    p.instance_id.clone(),
                    p.instance_name.clone(),
                ))
            })
            .collect();
        (announce, membership, clients)
    };

    if clients.is_empty() {
        return report;
    }

    let http = transport::shared_http_client();
    for (addresses, port, token, peer_id, peer_name) in &clients {
        report.sent += 1;
        let stale_marker: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let result = transport::try_peer(addresses, |addr| {
            let url = format!("http://{}:{}/api/mesh/announce", addr.ip, port);
            let http = http.clone();
            let token = token.clone();
            let announce = announce.clone();
            let membership = membership.clone();
            let stale_marker = stale_marker.clone();
            async move {
                let mut request = http
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .header(
                        crate::version::VERSION_HEADER,
                        crate::version::local_version(),
                    );
                if let Some(membership) = membership {
                    request = request
                        .header(crate::account_mesh::protocol::MEMBERSHIP_HEADER, membership);
                }
                match request.json(&announce).send().await {
                    Ok(resp) if resp.status().is_success() => transport::TryNext::Success(()),
                    Ok(resp) if resp.status().as_u16() == 404 => {
                        *stale_marker.lock() = true;
                        transport::TryNext::Stop("404 (too old)".to_string())
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        transport::TryNext::Stop(format!("HTTP {}", status))
                    }
                    Err(_) => transport::TryNext::Retry("request failed".to_string()),
                }
            }
        })
        .await;
        match result {
            Ok(()) => {
                report.ok += 1;
                mesh.lock().record_announce_outcome(peer_id, true);
                log::debug!(
                    "[mesh/announce] sent to {} ({}) address_count={}",
                    peer_name,
                    peer_id,
                    announce.addresses.len()
                );
            }
            Err(_e) if *stale_marker.lock() => {
                log::warn!(
                    "[mesh/announce] peer {} ({}) too old — no /api/mesh/announce",
                    peer_name,
                    peer_id
                );
                report.stale_peers.push(peer_id.clone());
            }
            Err(e) => {
                mesh.lock().record_announce_outcome(peer_id, false);
                let error = peer_request_error(peer_id, &e);
                log::warn!("[mesh/announce] send failed: {}", error);
                report.errors.push(error);
            }
        }
    }
    report
}

pub async fn refresh_local_addresses(mesh: &MeshState) -> bool {
    let observed_generation = refresh_generation().load(Ordering::Acquire);
    let mesh = mesh.clone();
    tokio::task::spawn_blocking(move || {
        refresh_local_addresses_guarded(&mesh, false, observed_generation)
    })
    .await
    .unwrap_or_else(|error| {
        log::error!("[mesh/refresh] address worker failed: {error}");
        false
    })
}

pub fn refresh_local_addresses_blocking(mesh: &MeshState) -> bool {
    let observed_generation = refresh_generation().load(Ordering::Acquire);
    refresh_local_addresses_guarded(mesh, false, observed_generation)
}

pub fn refresh_local_addresses_blocking_force(mesh: &MeshState) -> bool {
    refresh_local_addresses_guarded(mesh, true, 0)
}

fn refresh_local_addresses_guarded(
    mesh: &MeshState,
    force: bool,
    observed_generation: u64,
) -> bool {
    let _refresh_guard = match refresh_gate().lock() {
        Ok(guard) => guard,
        Err(_) => {
            log::error!("[mesh/refresh] refresh gate is poisoned");
            return false;
        }
    };
    if !force && refresh_generation().load(Ordering::Acquire) != observed_generation {
        return false;
    }
    let prev_ts = mesh.lock().tailscale_ip.clone();
    let prev_lan = mesh.lock().local_ip.clone();
    let prev_web_ui = mesh.lock().local_web_ui.clone();

    discovery::detect_tailscale(mesh);
    discovery::detect_local_ip(mesh);
    let (api_port, build_hash, local_http_healthy) = {
        let m = mesh.lock();
        (m.api_port, m.build_hash.clone(), m.local_http_healthy)
    };
    let snapshot =
        super::serve_setup::reconcile_web_serve(api_port, None, Some(build_hash.clone()));
    let https = matches!(
        snapshot.state,
        super::serve_setup::ServeReconcileState::Ready
    )
    .then_some(snapshot.url)
    .flatten();
    let new_ts = mesh.lock().tailscale_ip.clone();
    let new_lan = mesh.lock().local_ip.clone();
    let web_ui = local_http_healthy
        .then(|| {
            super::web_ui_endpoints::collect_verified(
                new_ts.as_deref(),
                new_lan.as_deref(),
                api_port,
                https.as_deref(),
                &build_hash,
            )
        })
        .flatten();
    mesh.lock().local_web_ui = web_ui.clone();
    let changed = prev_ts != new_ts || prev_lan != new_lan || prev_web_ui != web_ui;
    // actually changed; the steady state is debug to keep the log readable.
    if changed {
        log::info!("[mesh/refresh] local address sets changed");
    } else {
        log::debug!(
            "[mesh/refresh] local address sets: tailscale={}, lan={}, web_ui={}",
            new_ts.is_some(),
            new_lan.is_some(),
            web_ui.is_some()
        );
    }
    refresh_generation().fetch_add(1, Ordering::Release);
    changed
}

#[cfg(test)]
mod tests;
