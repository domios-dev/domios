use serde::Deserialize;
use serde_json::Value;

use crate::crown::AuthenticatedOrigin;
use crate::types::{is_supported_relay_protocol, PeerAnnounce, RelayCommand};

use super::types::{CallerContext, OutboundOperation, OutboundPeerRequest, OutboxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RouteTarget {
    pub method: &'static str,
    pub path: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationRouteBinding {
    pub outer: RouteTarget,
    pub inner: Option<RouteTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OuterWireRequest {
    pub route: RouteTarget,
    pub body: Value,
}

pub const OUTER_ANNOUNCE_ROUTE: RouteTarget = RouteTarget {
    method: "POST",
    path: "/api/mesh/announce",
};
pub const OUTER_RELAY_ROUTE: RouteTarget = RouteTarget {
    method: "POST",
    path: "/api/mesh/relay",
};
pub const OUTER_RPC_ROUTE: RouteTarget = RouteTarget {
    method: "POST",
    path: "/api/mesh/rpc",
};
pub const INNER_RPC_ROUTE: RouteTarget = RouteTarget {
    method: "POST",
    path: "/api/mesh/rpc",
};
pub const INNER_NOTIFICATION_ROUTE: RouteTarget = RouteTarget {
    method: "POST",
    path: "/api/notifications",
};

pub const WORKER_ACK_RPC_METHOD: &str = "plan.task.remote_ack";
pub const WORKER_REPORT_RPC_METHOD: &str = "plan.task.remote_report";

pub const fn route_binding(operation: OutboundOperation) -> OperationRouteBinding {
    match operation {
        OutboundOperation::Announce => OperationRouteBinding {
            outer: OUTER_ANNOUNCE_ROUTE,
            inner: None,
        },
        OutboundOperation::WorkerAck
        | OutboundOperation::WorkerReport
        | OutboundOperation::PlanRpc => OperationRouteBinding {
            outer: OUTER_RELAY_ROUTE,
            inner: Some(INNER_RPC_ROUTE),
        },
        OutboundOperation::Notification => OperationRouteBinding {
            outer: OUTER_RELAY_ROUTE,
            inner: Some(INNER_NOTIFICATION_ROUTE),
        },
    }
}

pub const fn direct_outer_route(operation: OutboundOperation) -> Option<RouteTarget> {
    match operation {
        OutboundOperation::Announce => Some(OUTER_ANNOUNCE_ROUTE),
        OutboundOperation::WorkerAck
        | OutboundOperation::WorkerReport
        | OutboundOperation::PlanRpc => Some(OUTER_RPC_ROUTE),
        OutboundOperation::Notification => None,
    }
}

pub fn is_allowed_plan_rpc_method(method: &str) -> bool {
    if matches!(
        method,
        "team.adopt_agent" | "team.close" | WORKER_ACK_RPC_METHOD | WORKER_REPORT_RPC_METHOD
    ) {
        return false;
    }
    matches!(
        method.split('.').next().unwrap_or_default(),
        "plan" | "spec" | "effort" | "task" | "team"
    )
}

#[derive(Deserialize)]
struct MeshRpcEnvelope {
    method: String,
    #[serde(default)]
    params: Option<Value>,
    request_id: String,
}

pub fn validate_operation_request(
    operation: OutboundOperation,
    caller_context: &CallerContext,
    body: &[u8],
) -> Result<Value, OutboxError> {
    match operation {
        OutboundOperation::Announce => validate_announce(caller_context, body),
        OutboundOperation::WorkerAck => validate_worker_ack(caller_context, body),
        OutboundOperation::WorkerReport => validate_worker_report(caller_context, body),
        OutboundOperation::Notification => validate_notification(caller_context, body),
        OutboundOperation::PlanRpc => validate_plan_rpc(caller_context, body),
    }
}

pub fn build_outer_wire_request(
    request: &OutboundPeerRequest,
) -> Result<OuterWireRequest, OutboxError> {
    request.validate()?;
    let inner_body =
        validate_operation_request(request.operation, &request.caller_context, &request.body)?;
    let binding = request.operation.route_binding();
    match binding.inner {
        None => Ok(OuterWireRequest {
            route: binding.outer,
            body: inner_body,
        }),
        Some(inner) => {
            let cmd = RelayCommand {
                method: inner.method.to_string(),
                path: inner.path.to_string(),
                body: inner_body,
                delivery_id: None,
            };
            let body = serde_json::to_value(cmd).map_err(|error| OutboxError::InvalidRequest {
                reason: format!("failed to build outer relay envelope: {error}"),
            })?;
            Ok(OuterWireRequest {
                route: binding.outer,
                body,
            })
        }
    }
}

fn validate_announce(caller: &CallerContext, body: &[u8]) -> Result<Value, OutboxError> {
    require_owner_trusted_only(caller, "announce")?;
    let announce: PeerAnnounce =
        serde_json::from_slice(body).map_err(|error| OutboxError::InvalidRequest {
            reason: format!("announce body must be valid PeerAnnounce JSON: {error}"),
        })?;
    if announce.instance_id.trim().is_empty() {
        return Err(OutboxError::InvalidRequest {
            reason: "announce body instance_id must be non-empty".into(),
        });
    }
    if !is_supported_relay_protocol(announce.relay_protocol_version) {
        return Err(OutboxError::InvalidRequest {
            reason: format!(
                "announce relay_protocol_version {} is unsupported",
                announce.relay_protocol_version
            ),
        });
    }
    parse_json_object(body, "announce")
}

fn validate_worker_ack(caller: &CallerContext, body: &[u8]) -> Result<Value, OutboxError> {
    require_owner_or_agent(caller, "worker_ack")?;
    let (raw, rpc) = parse_rpc_envelope(body, "worker_ack")?;
    if rpc.method != WORKER_ACK_RPC_METHOD {
        return Err(OutboxError::InvalidRequest {
            reason: format!(
                "worker_ack requires RPC method '{WORKER_ACK_RPC_METHOD}', got '{}'",
                rpc.method
            ),
        });
    }
    let params = require_rpc_params_object(&rpc.params, "worker_ack")?;
    reject_server_injected_rpc_fields(params, "worker_ack")?;
    require_nonempty_str_field(params, "plan_id", "worker_ack")?;
    require_nonempty_str_field(params, "task_id", "worker_ack")?;
    require_nonempty_str_field(params, "attempt_id", "worker_ack")?;
    let worker_tab_id = require_nonempty_str_field(params, "worker_tab_id", "worker_ack")?;
    if caller.origin == AuthenticatedOrigin::Agent {
        let agent = caller
            .agent
            .as_ref()
            .ok_or_else(|| OutboxError::InvalidRequest {
                reason: "agent worker_ack requires authenticated agent context".into(),
            })?;
        if worker_tab_id != agent.tab_id {
            return Err(OutboxError::InvalidRequest {
                reason: "worker_ack worker_tab_id must match originating agent tab_id".into(),
            });
        }
    }
    Ok(raw)
}

fn validate_worker_report(caller: &CallerContext, body: &[u8]) -> Result<Value, OutboxError> {
    require_owner_or_agent(caller, "worker_report")?;
    let (raw, rpc) = parse_rpc_envelope(body, "worker_report")?;
    if rpc.method != WORKER_REPORT_RPC_METHOD {
        return Err(OutboxError::InvalidRequest {
            reason: format!(
                "worker_report requires RPC method '{WORKER_REPORT_RPC_METHOD}', got '{}'",
                rpc.method
            ),
        });
    }
    let params = require_rpc_params_object(&rpc.params, "worker_report")?;
    reject_server_injected_rpc_fields(params, "worker_report")?;
    require_nonempty_str_field(params, "plan_id", "worker_report")?;
    require_nonempty_str_field(params, "task_id", "worker_report")?;
    require_nonempty_str_field(params, "attempt_id", "worker_report")?;
    let report = params
        .get("report")
        .and_then(Value::as_object)
        .ok_or_else(|| OutboxError::InvalidRequest {
            reason: "worker_report params.report must be a JSON object".into(),
        })?;
    require_nonempty_str_field(report, "id", "worker_report.report")?;
    require_nonempty_str_field(report, "message", "worker_report.report")?;
    let status = require_nonempty_str_field(report, "status", "worker_report.report")?;
    if status != "in_review" {
        return Err(OutboxError::InvalidRequest {
            reason: "worker_report report.status must be 'in_review'".into(),
        });
    }
    let reporter_tab_id =
        require_nonempty_str_field(report, "reporter_tab_id", "worker_report.report")?;
    if let Some(worker_tab_id) = params.get("worker_tab_id").and_then(Value::as_str) {
        if worker_tab_id != reporter_tab_id {
            return Err(OutboxError::InvalidRequest {
                reason: "worker_report worker_tab_id must match report.reporter_tab_id".into(),
            });
        }
    }
    if caller.origin == AuthenticatedOrigin::Agent {
        let agent = caller
            .agent
            .as_ref()
            .ok_or_else(|| OutboxError::InvalidRequest {
                reason: "agent worker_report requires authenticated agent context".into(),
            })?;
        if reporter_tab_id != agent.tab_id {
            return Err(OutboxError::InvalidRequest {
                reason: "worker_report report.reporter_tab_id must match originating agent tab_id"
                    .into(),
            });
        }
    }
    Ok(raw)
}

fn validate_notification(caller: &CallerContext, body: &[u8]) -> Result<Value, OutboxError> {
    require_owner_or_agent(caller, "notification")?;
    let raw = parse_json_object(body, "notification")?;
    let Some(obj) = raw.as_object() else {
        return Err(OutboxError::InvalidRequest {
            reason: "notification body must be a JSON object".into(),
        });
    };
    require_nonempty_str_field(obj, "title", "notification")?;
    for forbidden in ["source_peer", "tab_id", "workspace_id"] {
        if obj.get(forbidden).is_some_and(|value| !value.is_null()) {
            return Err(OutboxError::InvalidRequest {
                reason: format!("notification body must not supply '{forbidden}'"),
            });
        }
    }
    Ok(raw)
}

fn validate_plan_rpc(caller: &CallerContext, body: &[u8]) -> Result<Value, OutboxError> {
    require_owner_trusted_only(caller, "plan_rpc")?;
    let (raw, rpc) = parse_rpc_envelope(body, "plan_rpc")?;
    if !is_allowed_plan_rpc_method(&rpc.method) {
        return Err(OutboxError::InvalidRequest {
            reason: format!("plan_rpc method '{}' is not permitted", rpc.method),
        });
    }
    if let Some(params) = rpc.params.as_ref().and_then(Value::as_object) {
        reject_server_injected_rpc_fields(params, "plan_rpc")?;
    }
    Ok(raw)
}

fn require_owner_trusted_only(caller: &CallerContext, op: &str) -> Result<(), OutboxError> {
    if !caller.origin.owner_trusted() {
        return Err(OutboxError::InvalidRequest {
            reason: format!("{op} requires an owner-trusted origin (Owner or Mesh)"),
        });
    }
    if caller.agent.is_some() {
        return Err(OutboxError::InvalidRequest {
            reason: format!("{op} must not carry an agent identity"),
        });
    }
    Ok(())
}

fn require_owner_or_agent(caller: &CallerContext, op: &str) -> Result<(), OutboxError> {
    match caller.origin {
        AuthenticatedOrigin::Owner | AuthenticatedOrigin::Mesh => {
            if caller.agent.is_some() {
                return Err(OutboxError::InvalidRequest {
                    reason: format!("owner-trusted {op} must not carry an agent identity"),
                });
            }
            Ok(())
        }
        AuthenticatedOrigin::Agent => {
            if caller.agent.is_none() {
                return Err(OutboxError::InvalidRequest {
                    reason: format!("agent {op} requires caller_context.agent"),
                });
            }
            Ok(())
        }
        AuthenticatedOrigin::Service => Err(OutboxError::InvalidRequest {
            reason: format!("service origin is not permitted for {op}"),
        }),
    }
}

fn parse_json_object(body: &[u8], op: &str) -> Result<Value, OutboxError> {
    let value: Value =
        serde_json::from_slice(body).map_err(|error| OutboxError::InvalidRequest {
            reason: format!("{op} body must be valid JSON: {error}"),
        })?;
    if !value.is_object() {
        return Err(OutboxError::InvalidRequest {
            reason: format!("{op} body must be a JSON object"),
        });
    }
    Ok(value)
}

fn parse_rpc_envelope(body: &[u8], op: &str) -> Result<(Value, MeshRpcEnvelope), OutboxError> {
    let raw = parse_json_object(body, op)?;
    let rpc: MeshRpcEnvelope =
        serde_json::from_value(raw.clone()).map_err(|error| OutboxError::InvalidRequest {
            reason: format!("{op} body must be a valid MeshRpcRequest: {error}"),
        })?;
    if rpc.request_id.is_empty()
        || rpc.request_id.len() > 128
        || rpc.request_id.chars().any(char::is_control)
    {
        return Err(OutboxError::InvalidRequest {
            reason: format!("{op} request_id must be 1..=128 non-control characters"),
        });
    }
    Ok((raw, rpc))
}

fn require_rpc_params_object<'a>(
    params: &'a Option<Value>,
    op: &str,
) -> Result<&'a serde_json::Map<String, Value>, OutboxError> {
    params
        .as_ref()
        .and_then(Value::as_object)
        .ok_or_else(|| OutboxError::InvalidRequest {
            reason: format!("{op} params must be a JSON object"),
        })
}

fn reject_server_injected_rpc_fields(
    params: &serde_json::Map<String, Value>,
    op: &str,
) -> Result<(), OutboxError> {
    for forbidden in ["authenticated_mesh_peer_id", "caller_tab_id"] {
        if params.contains_key(forbidden) {
            return Err(OutboxError::InvalidRequest {
                reason: format!("{op} params must not supply '{forbidden}'"),
            });
        }
    }
    Ok(())
}

fn require_nonempty_str_field<'a>(
    obj: &'a serde_json::Map<String, Value>,
    field: &str,
    context: &str,
) -> Result<&'a str, OutboxError> {
    obj.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| OutboxError::InvalidRequest {
            reason: format!("{context}.{field} must be a non-empty string"),
        })
}
