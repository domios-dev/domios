use axum::http::{HeaderMap, StatusCode};
use codeterm_service_core::{
    mesh::crown::{AuthenticatedAgent, AuthenticatedOrigin},
    service::cloud_machines::{CloudLifecycleState, DEFAULT_WAKE_HOLD_DEADLINE_MS},
};

use super::{
    is_tab_send, origin, relay_failure, relay_send_delivery_id, AnsweringPeer, PeerRelayCommand,
    PeerRelayResponse,
};
use crate::routes::AppState;

fn tab_id_from_path(path: &str) -> String {
    let bare = path.split('?').next().unwrap_or(path);
    bare.strip_prefix("/api/tabs/")
        .and_then(|rest| rest.split('/').next())
        .filter(|id| !id.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn is_machine_sleeping_refusal(resp: &codeterm_service_core::mesh::types::RelayResponse) -> bool {
    !resp.ok
        && resp
            .data
            .as_ref()
            .and_then(|d| d.pointer("/error/code"))
            .and_then(serde_json::Value::as_str)
            == Some("machine_sleeping")
        && resp
            .data
            .as_ref()
            .and_then(|d| d.pointer("/error/bytes_written"))
            .and_then(serde_json::Value::as_bool)
            == Some(false)
}

fn known_peer_lacks_require_agent(state: &AppState, raw_target: &str) -> Option<String> {
    let mesh = state.mesh.lock();
    let peer = mesh.resolve_peer_including_offline(raw_target).ok()?;
    if peer.relay_protocol_version > 0
        && !codeterm_service_core::mesh::types::is_supported_relay_protocol(
            peer.relay_protocol_version,
        )
    {
        return Some(peer.instance_name.clone());
    }
    None
}

pub(super) async fn preflight_cloud_wake(
    state: &AppState,
    headers: &HeaderMap,
    req: &PeerRelayCommand,
) -> Result<Option<PeerRelayResponse>, (StatusCode, String)> {
    let raw_target = req
        .target_instance_id
        .as_deref()
        .or(req.target_peer_id.as_deref())
        .unwrap_or_default();
    let Some(machine) = state.svc.cloud_machines().find_by_peer(raw_target) else {
        return Ok(None);
    };
    let peer = AnsweringPeer {
        instance_id: machine
            .peer_instance_id
            .clone()
            .unwrap_or_else(|| machine.machine_id.clone()),
        instance_name: machine.name.clone(),
    };
    if !machine.state.needs_wake_for_use() && machine.state != CloudLifecycleState::OutOfCredits {
        return Ok(None);
    }

    // Authenticate and sign the local caller before checking credits, holding, or waking so
    // unauthenticated callers never trigger backend probes or escalate privilege.
    let (caller_origin, caller_agent, _) =
        crate::auth::off_runtime(state, headers, origin::signed_caller_token).await?;
    if caller_origin == AuthenticatedOrigin::Owner {
        state
            .svc
            .cloud_machines()
            .record_owner_interaction(&machine.machine_id);
    }

    if is_tab_send(&req.method, &req.path) {
        if req.body["require_agent"].as_bool() == Some(true) {
            if let Some(peer_name) = known_peer_lacks_require_agent(state, raw_target) {
                return Ok(Some(PeerRelayResponse {
                    response: relay_failure(
                        StatusCode::UPGRADE_REQUIRED.as_u16(),
                        "peer_lacks_agent_only_send",
                        Some(false),
                        format!(
                            "Cannot verify agent-only delivery on {peer_name}. Check that it is reachable and running an updated Domios."
                        ),
                    ),
                    peer,
                }));
            }
        }
        crate::mesh_attachment::guard_remote_send_on_host(&req.method, &req.path, &req.body)
            .await
            .map_err(|error| (StatusCode::UNPROCESSABLE_ENTITY, error))?;
        if machine.state == CloudLifecycleState::OutOfCredits {
            if let Err(error) =
                crate::cloud_routes::check_credits_for_wake(state, &machine.machine_id).await
            {
                return Ok(Some(PeerRelayResponse {
                    response: relay_failure(
                        crate::cloud_routes::error_status(&error).as_u16(),
                        error.code(),
                        Some(false),
                        error.to_string(),
                    ),
                    peer,
                }));
            }
        }
        let tab_id = tab_id_from_path(&req.path);
        let delivery_id = relay_send_delivery_id(&req.method, &req.path)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let held = state
            .svc
            .cloud_machines()
            .hold_send_for_wake(
                &machine.machine_id,
                &tab_id,
                delivery_id,
                req.method.clone(),
                req.path.clone(),
                req.body.clone(),
                caller_origin,
                caller_agent,
                DEFAULT_WAKE_HOLD_DEADLINE_MS,
            )
            .map_err(|error| (StatusCode::CONFLICT, error.to_string()))?;
        crate::cloud_routes::spawn_wake_and_flush(state.clone(), machine.machine_id.clone());
        return Ok(Some(PeerRelayResponse {
            response: codeterm_service_core::mesh::types::RelayResponse {
                ok: true,
                status: StatusCode::ACCEPTED.as_u16(),
                data: Some(serde_json::json!({
                    "ok": true,
                    "held": true,
                    "bytes_written": false,
                    "send_id": held.send_id,
                    "release_condition": held.release_condition,
                    "deadline_ms": held.deadline_ms,
                    "machine_id": machine.machine_id,
                })),
                error: None,
            },
            peer,
        }));
    }
    if let Err(error) = crate::cloud_routes::wake_machine_for_use(state, &machine.machine_id).await
    {
        return Ok(Some(PeerRelayResponse {
            response: relay_failure(
                crate::cloud_routes::error_status(&error).as_u16(),
                error.code(),
                Some(false),
                error.to_string(),
            ),
            peer,
        }));
    }
    Ok(None)
}

pub(super) fn hold_if_drain_raced(
    state: &AppState,
    peer_id: &str,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    resp: &codeterm_service_core::mesh::types::RelayResponse,
    origin: AuthenticatedOrigin,
    agent: Option<AuthenticatedAgent>,
) -> Option<codeterm_service_core::mesh::types::RelayResponse> {
    if !is_tab_send(&cmd.method, &cmd.path) || !is_machine_sleeping_refusal(resp) {
        return None;
    }
    let machine = state.svc.cloud_machines().find_by_peer(peer_id)?;
    let tab_id = tab_id_from_path(&cmd.path);
    let delivery_id = cmd
        .delivery_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let held = state
        .svc
        .cloud_machines()
        .hold_send_for_wake(
            &machine.machine_id,
            &tab_id,
            delivery_id,
            cmd.method.clone(),
            cmd.path.clone(),
            cmd.body.clone(),
            origin,
            agent,
            DEFAULT_WAKE_HOLD_DEADLINE_MS,
        )
        .ok()?;
    crate::cloud_routes::spawn_wake_and_flush(state.clone(), machine.machine_id.clone());
    Some(codeterm_service_core::mesh::types::RelayResponse {
        ok: true,
        status: StatusCode::ACCEPTED.as_u16(),
        data: Some(serde_json::json!({
            "ok": true,
            "held": true,
            "bytes_written": false,
            "send_id": held.send_id,
            "release_condition": held.release_condition,
            "deadline_ms": held.deadline_ms,
            "machine_id": machine.machine_id,
        })),
        error: None,
    })
}
