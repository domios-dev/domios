use super::*;
use codeterm_service_core::service::AuthenticatedPlanCaller;

pub(super) fn dispatch(
    method: &str,
    params: &Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    let body = params
        .as_ref()
        .ok_or_else(|| error("params required".into()))?;
    let caller = AuthenticatedPlanCaller {
        tab_id: body["caller_tab_id"].as_str().map(str::to_string),
        session_id: body["session_id"].as_str().map(str::to_string),
        binding_nonce: body["session_binding_nonce"].as_str().map(str::to_string),
    };
    let now = svc.curator_now().map_err(error)?;
    if method == "plan.curator.finish" {
        let note = get_string_param(params, "note")?;
        let seconds = body["next_secs"]
            .as_u64()
            .ok_or_else(|| error("next_secs required".into()))?;
        return svc
            .curator_finish(&caller, &note, seconds, now)
            .map(|round| serde_json::json!(round))
            .map_err(error);
    }
    let id = super::plan_refs::resolve_plan(&svc.plan_store, params)?;
    match method {
        "plan.control_loop.run" => svc
            .control_loop_run(&id, &caller, now)
            .map(|value| serde_json::json!(value))
            .map_err(error),
        "plan.control_loop.show" => svc
            .control_loop_show(&id)
            .map(|v| serde_json::json!(v))
            .map_err(error),
        "plan.control_loop.set" => {
            let update = serde_json::from_value(body.clone())
                .map_err(|e| error(format!("invalid control loop: {e}")))?;
            svc.control_loop_set(&id, &caller, update, now)
                .map(|v| serde_json::json!(v))
                .map_err(error)
        }
        "plan.curator.grant" => {
            let tab = get_string_param(params, "curator_tab")?;
            let blueprints = serde_json::from_value(
                body.get("blueprints")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .map_err(|e| error(e.to_string()))?;
            let skills = serde_json::from_value(
                body.get("plan_skills")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .map_err(|e| error(e.to_string()))?;
            let saves = serde_json::from_value(
                body.get("saves")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .map_err(|e| error(e.to_string()))?;
            svc.curator_grant(
                &id,
                &caller,
                &tab,
                codeterm_service_core::service::curator::CuratorGrantScope {
                    blueprints,
                    plan_skills: skills,
                    saves,
                },
                now,
            )
            .map(|v| serde_json::json!(v))
            .map_err(error)
        }
        _ => Err(error("unknown Curator method".into())),
    }
}

fn error(message: String) -> JsonRpcError {
    JsonRpcError {
        code: -32602,
        message,
        data: None,
    }
}
