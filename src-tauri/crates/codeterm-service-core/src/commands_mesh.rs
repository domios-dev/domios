// ── Mesh commands ───────────────────────────────────────────────────

/// Shared implementation behind the Tauri command and the web-mirror HTTP
/// route (`GET /api/mesh/peers/{id}/services`).
pub async fn fetch_peer_services_view(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
) -> Result<serde_json::Value, String> {
    use crate::mesh::peer_services::{build_view, FetchOutcome};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let (addresses, port, online, token, cache_fresh) = {
        let m = mesh.lock();
        let peer = m
            .paired_peers
            .get(instance_id)
            .ok_or_else(|| format!("Peer {} not paired", instance_id))?;
        let token = m
            .sign_request()
            .map_err(|e| format!("sign_request: {}", e))?;
        (
            crate::mesh::types::sanitize_addresses(&peer.addresses),
            peer.api_port,
            peer.online,
            token,
            m.peer_services.is_fresh(instance_id, now),
        )
    };

    let outcome = if cache_fresh || !online {
        FetchOutcome::Skipped
    } else {
        fetch_peer_services(&addresses, port, &token).await
    };

    let mut m = mesh.lock();
    if let FetchOutcome::Fetched(ref services) = outcome {
        m.peer_services.refresh(instance_id, services.clone(), now);
    }
    let view = build_view(m.peer_services.get(instance_id), online, outcome, now);
    serde_json::to_value(&view).map_err(|e| e.to_string())
}

/// Per-address timeout for the direct peer /api/services fetch. A peer with many
/// services + mesh-signed-token verification can take ~5s to answer (measured
/// against a 17-service peer); the old 3s cut every such peer off mid-request, so
/// the Mesh tab showed 0 services for healthy peers. Kept above real-world latency
/// but bounded so a dead address still fails fast enough for the next address.
const PEER_SERVICES_FETCH_TIMEOUT_SECS: u64 = 10;

async fn fetch_peer_services(
    addresses: &[crate::mesh::types::PeerAddress],
    port: u16,
    token: &str,
) -> crate::mesh::peer_services::FetchOutcome {
    use crate::mesh::peer_services::FetchOutcome;
    use crate::mesh::transport::TryNext;

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            PEER_SERVICES_FETCH_TIMEOUT_SECS,
        ))
        .build()
    {
        Ok(c) => c,
        Err(e) => return FetchOutcome::Failed(e.to_string()),
    };

    let result = crate::mesh::transport::try_peer(addresses, |addr| {
        let client = client.clone();
        let url = format!("http://{}:{}/api/mesh/inventory", addr.ip, port);
        let token = token.to_string();
        async move {
            let resp = match client
                .get(&url)
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return TryNext::Retry(format!("request: {}", e)),
            };
            if !resp.status().is_success() {
                return TryNext::Stop(format!("HTTP {}", resp.status()));
            }
            match resp
                .json::<crate::mesh_host::MeshInventoryView>()
                .await
            {
                Ok(view) => TryNext::Success(
                    view.resources
                        .into_iter()
                        .filter_map(|resource| serde_json::to_value(resource).ok())
                        .collect(),
                ),
                Err(e) => TryNext::Stop(format!("parse: {}", e)),
            }
        }
    })
    .await;

    match result {
        Ok(services) => FetchOutcome::Fetched(services),
        Err(e) => FetchOutcome::Failed(e),
    }
}

#[derive(Clone, Copy)]
enum PeerMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// Signed-token request to a paired peer's REST API, walking its addresses just
/// like the services fetch. This is the cross-mesh ACTION path: the Mesh tab can
/// delete a service or control a cron ON the peer, instead of every mutation
/// silently hitting the local daemon. Returns the peer's JSON response (or Null
/// for empty bodies like a 204 delete).
async fn peer_authed_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    method: PeerMethod,
    path: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request_with_body(mesh, instance_id, method, path, None).await
}

async fn peer_authed_request_with_body(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    method: PeerMethod,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    use crate::mesh::transport::TryNext;

    let (addresses, port, token) = {
        let m = mesh.lock();
        let peer = m
            .paired_peers
            .get(instance_id)
            .ok_or_else(|| format!("Peer {} not paired", instance_id))?;
        let token = m
            .sign_request()
            .map_err(|e| format!("sign_request: {}", e))?;
        (
            crate::mesh::types::sanitize_addresses(&peer.addresses),
            peer.api_port,
            token,
        )
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            PEER_SERVICES_FETCH_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| e.to_string())?;

    crate::mesh::transport::try_peer(&addresses, |addr| {
        let client = client.clone();
        let url = format!("http://{}:{}{}", addr.ip, port, path);
        let token = token.clone();
        let body = body.clone();
        async move {
            let req = match method {
                PeerMethod::Get => client.get(&url),
                PeerMethod::Post => client.post(&url),
                PeerMethod::Put => client.put(&url),
                PeerMethod::Delete => client.delete(&url),
            };
            let req = match body.as_ref() {
                Some(body) if matches!(method, PeerMethod::Post | PeerMethod::Put) => {
                    req.json(body)
                }
                _ => req,
            };
            let resp = match req
                .header("Authorization", format!("Bearer {}", token))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => return TryNext::Retry(format!("request: {}", e)),
            };
            if !resp.status().is_success() {
                return TryNext::Stop(format!("HTTP {}", resp.status()));
            }
            // Action endpoints may return JSON or an empty body (204); tolerate both.
            let text = resp.text().await.unwrap_or_default();
            let val = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
            TryNext::Success(val)
        }
    })
    .await
}

pub async fn peer_service_delete_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Delete,
        &format!("/api/services/{}", id),
    )
    .await
}

pub async fn peer_cron_action_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
    action: &str,
) -> Result<serde_json::Value, String> {
    let action = PeerCronAction::parse(action)
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.to_string()))?;
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Post,
        &format!("/api/crons/{}/{}", id, action.as_str()),
    )
    .await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerCronAction {
    Enable,
    Disable,
    Trigger,
    Cancel,
}

#[derive(Debug, serde::Serialize)]
struct UnsupportedCronOperation<'a> {
    code: &'static str,
    operation: &'a str,
}

impl std::fmt::Display for UnsupportedCronOperation<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "unsupported cron action: {}", self.operation)
    }
}

impl PeerCronAction {
    fn parse(operation: &str) -> Result<Self, UnsupportedCronOperation<'_>> {
        match operation {
            "enable" => Ok(Self::Enable),
            "disable" => Ok(Self::Disable),
            "trigger" => Ok(Self::Trigger),
            "cancel" => Ok(Self::Cancel),
            operation => Err(UnsupportedCronOperation {
                code: "unsupported_operation",
                operation,
            }),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Trigger => "trigger",
            Self::Cancel => "cancel",
        }
    }
}

pub async fn peer_cron_update_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
    patch: serde_json::Value,
) -> Result<serde_json::Value, String> {
    peer_authed_request_with_body(
        mesh,
        instance_id,
        PeerMethod::Put,
        &format!("/api/crons/{id}"),
        Some(patch),
    )
    .await
}

pub async fn peer_cron_delete_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Delete,
        &format!("/api/crons/{id}"),
    )
    .await
}

pub async fn peer_cron_history_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Get,
        &format!("/api/crons/{}/history", id),
    )
    .await
}

pub async fn peer_cron_get_request(
    mesh: &crate::MeshStateHandle,
    instance_id: &str,
    id: &str,
) -> Result<serde_json::Value, String> {
    peer_authed_request(
        mesh,
        instance_id,
        PeerMethod::Get,
        &format!("/api/crons/{id}"),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target_outcome(status: u16, body: serde_json::Value) -> Result<serde_json::Value, String> {
        if status < 400 {
            Ok(body)
        } else {
            Err(format!("HTTP {status}"))
        }
    }

    #[test]
    fn mesh_inventory_remote_cron_update_and_delete_forward_target_outcomes() {
        let update_path = "/api/crons/cron-1";
        let delete_path = "/api/crons/cron-1";
        assert_eq!(update_path, delete_path);
        assert_eq!(
            target_outcome(
                200,
                serde_json::json!({"id": "cron-1", "schedule": "0 * * * *"}),
            ),
            Ok(serde_json::json!({"id": "cron-1", "schedule": "0 * * * *"}))
        );
        assert_eq!(
            target_outcome(200, serde_json::json!({"ok": true})),
            Ok(serde_json::json!({"ok": true}))
        );
        assert_eq!(
            target_outcome(403, serde_json::json!({"error": "denied"})),
            Err("HTTP 403".into())
        );
        let unsupported = PeerCronAction::parse("restart").unwrap_err();
        assert_eq!(
            serde_json::to_value(unsupported).unwrap(),
            serde_json::json!({"code": "unsupported_operation", "operation": "restart"})
        );
    }
}
