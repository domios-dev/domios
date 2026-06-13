use std::sync::Arc;

use parking_lot::Mutex;

use super::MeshState;
use super::{discovery, transport, types};

pub async fn broadcast_sync(mesh: &MeshState) {
    let (payload, clients) = {
        let m = mesh.lock();
        let payload = m.build_sync_payload();
        let clients: Vec<_> = m
            .paired_peers
            .values()
            .filter_map(|p| {
                if p.addresses.is_empty() {
                    return None;
                }
                let token = m.sign_request().ok()?;
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
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
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
                        let body = resp.text().await.unwrap_or_default();
                        transport::TryNext::Stop(format!("HTTP {}: {}", status, body))
                    }
                    Err(e) => transport::TryNext::Retry(e.to_string()),
                }
            }
        })
        .await;
        match result {
            Ok(()) => log::info!("[mesh/broadcast] Synced to {}", peer_id),
            Err(e) => log::warn!("[mesh/broadcast] Sync failed for {}: {}", peer_id, e),
        }
    }
}

pub async fn broadcast_introductions(
    new_peer: types::PairedPeer,
    existing_peers: Vec<types::PairedPeer>,
    our_id: String,
    token: String,
) {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    for existing in &existing_peers {
        if !existing.addresses.is_empty() {
            let intro = types::PeerIntroduction {
                instance_id: new_peer.instance_id.clone(),
                instance_name: new_peer.instance_name.clone(),
                addresses: new_peer.addresses.clone(),
                api_port: new_peer.api_port,
                public_key: new_peer.public_key.clone(),
                introduced_by: our_id.clone(),
            };
            let result = transport::try_peer(&existing.addresses, |addr| {
                let url = format!(
                    "http://{}:{}/api/mesh/introduce",
                    addr.ip, existing.api_port
                );
                let http = http.clone();
                let token = token.clone();
                let intro = intro.clone();
                async move {
                    match http
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .json(&intro)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => transport::TryNext::Success(()),
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            transport::TryNext::Stop(format!("HTTP {}: {}", status, body))
                        }
                        Err(e) => transport::TryNext::Retry(e.to_string()),
                    }
                }
            })
            .await;
            match result {
                Ok(()) => log::info!(
                    "[mesh/introduce] {} → {}",
                    new_peer.instance_name,
                    existing.instance_name
                ),
                Err(e) => log::warn!(
                    "[mesh/introduce] {} → {} failed: {}",
                    new_peer.instance_name,
                    existing.instance_name,
                    e
                ),
            }
        }
        if !new_peer.addresses.is_empty() {
            let intro = types::PeerIntroduction {
                instance_id: existing.instance_id.clone(),
                instance_name: existing.instance_name.clone(),
                addresses: existing.addresses.clone(),
                api_port: existing.api_port,
                public_key: existing.public_key.clone(),
                introduced_by: our_id.clone(),
            };
            let result = transport::try_peer(&new_peer.addresses, |addr| {
                let url = format!(
                    "http://{}:{}/api/mesh/introduce",
                    addr.ip, new_peer.api_port
                );
                let http = http.clone();
                let token = token.clone();
                let intro = intro.clone();
                async move {
                    match http
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", token))
                        .json(&intro)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => transport::TryNext::Success(()),
                        Ok(resp) => {
                            let status = resp.status();
                            let body = resp.text().await.unwrap_or_default();
                            transport::TryNext::Stop(format!("HTTP {}: {}", status, body))
                        }
                        Err(e) => transport::TryNext::Retry(e.to_string()),
                    }
                }
            })
            .await;
            match result {
                Ok(()) => log::info!(
                    "[mesh/introduce] {} → {}",
                    existing.instance_name,
                    new_peer.instance_name
                ),
                Err(e) => log::warn!(
                    "[mesh/introduce] {} → {} failed: {}",
                    existing.instance_name,
                    new_peer.instance_name,
                    e
                ),
            }
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

pub async fn broadcast_announce(mesh: &MeshState, target: Option<&str>) -> AnnounceReport {
    let mut report = AnnounceReport::default();
    let (announce, clients) = {
        let m = mesh.lock();
        let announce = m.build_announce();
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
        (announce, clients)
    };

    if clients.is_empty() {
        return report;
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap_or_default();
    for (addresses, port, token, peer_id, peer_name) in &clients {
        report.sent += 1;
        let stale_marker: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let result = transport::try_peer(addresses, |addr| {
            let url = format!("http://{}:{}/api/mesh/announce", addr.ip, port);
            let http = http.clone();
            let token = token.clone();
            let announce = announce.clone();
            let stale_marker = stale_marker.clone();
            async move {
                match http
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&announce)
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => transport::TryNext::Success(()),
                    Ok(resp) if resp.status().as_u16() == 404 => {
                        *stale_marker.lock() = true;
                        transport::TryNext::Stop("404 (too old)".to_string())
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        transport::TryNext::Stop(format!("HTTP {}: {}", status, body))
                    }
                    Err(e) => transport::TryNext::Retry(e.to_string()),
                }
            }
        })
        .await;
        match result {
            Ok(()) => {
                report.ok += 1;
                log::info!(
                    "[mesh/announce] sent to {} ({}) addrs={:?}",
                    peer_name,
                    peer_id,
                    announce.addresses.iter().map(|a| &a.ip).collect::<Vec<_>>()
                );
            }
            Err(e) if *stale_marker.lock() => {
                log::warn!(
                    "[mesh/announce] peer {} ({}) too old — no /api/mesh/announce",
                    peer_name,
                    peer_id
                );
                report.stale_peers.push(peer_id.clone());
            }
            Err(e) => {
                log::warn!("[mesh/announce] {} send failed: {}", peer_id, e);
                report.errors.push(format!("{}: {}", peer_id, e));
            }
        }
    }
    report
}

pub fn refresh_local_addresses(mesh: &MeshState) -> bool {
    let prev_ts = mesh.lock().tailscale_ip.clone();
    let prev_lan = mesh.lock().local_ip.clone();
    let prev_https = mesh.lock().local_https_url.clone();

    discovery::detect_tailscale(mesh);
    discovery::detect_local_ip(mesh);
    let https = discovery::tailscale_serve_https_url(crate::config::load_config().api.port);
    mesh.lock().local_https_url = https.clone();

    let new_ts = mesh.lock().tailscale_ip.clone();
    let new_lan = mesh.lock().local_ip.clone();
    let changed = prev_ts != new_ts || prev_lan != new_lan || prev_https != https;
    log::info!(
        "[mesh/refresh] local addrs: tailscale {:?} -> {:?}, lan {:?} -> {:?}, https {:?} -> {:?}{}",
        prev_ts,
        new_ts,
        prev_lan,
        new_lan,
        prev_https,
        https,
        if changed { " (CHANGED)" } else { "" }
    );
    changed
}
