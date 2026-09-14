use super::*;
use tauri::Manager;

pub(super) fn dispatch(
    method: &str,
    params: Option<serde_json::Value>,
    mux: &MuxState,
    _pty: &PtyState,
    _notif: &NotifState,
    app: &AppHandle,
    _output_buf: &OutputBufferState,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    // System methods are tiny — handle inline.
    match method {
        "api.request" => return local_api::dispatch(&params, app),
        "system.identify" => {
            return Ok(serde_json::json!({
                "name": "CodeTerm",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
                "features": ["multiplexer", "agent-detection", "notifications"]
            }));
        }
        "system.capabilities" => {
            let methods = vec![
                "system.identify",
                "system.capabilities",
                "api.request",
                "workspace.list",
                "workspace.create",
                "workspace.focus",
                "workspace.rename",
                "workspace.close",
                "workspace.move_tab",
                "workspace.recently_closed",
                "workspace.reopen_closed",
                "workspace.reopen_closed_group",
                "tab.list",
                "tab.inspect",
                "tab.agent_pad",
                "tab.focus",
                "tab.close",
                "tab.send_text",
                "tab.get_output",
                "tab.rename",
                "tab.resize",
                "tab.kill_pty",
                "notification.create",
                "notification.list",
                "notification.clear",
                "mesh.status",
                "mesh.peers",
                "agent.list",
                "agent.hierarchy",
                "agent.spawn",
                "agent.set_role",
                "agent.clear_role",
                "agent.status",
                "agent.children",
                "agent.link",
                "agent.report",
                "agent.ack",
                "orchestration.new",
                "persona.list",
                "persona.show",
                "persona.reload",
                "team.list",
                "team.show",
                "team.rename",
                "team.close",
                "team.adopt_plan",
                "team.adopt_agent",
                "team.add_member",
                "orchestrator.refresh",
                "plan.create",
                "plan.adopt",
                "plan.binding.resolve",
                "plan.batch",
                "plan.handoff.create",
                "plan.handoff.list",
                "plan.handoff.show",
                "plan.handoff.reseed",
                "plan.list",
                "plan.get",
                "plan.goal.set",
                "plan.goal.show",
                "plan.spec",
                "plan.files",
                "plan.tags",
                "plan.update",
                "plan.mode.set",
                "plan.planning.set",
                "plan.session.rebind",
                "plan.archive",
                "plan.delete",
                "plan.transfer",
                "plan.disown",
                "plan.claim",
                "plan.ingest",
                "plan.section.add",
                "plan.section.update",
                "plan.section.attach",
                "plan.section.rm",
                "plan.kinds.set",
                "plan.tag_facets.set",
                "plan.task.add",
                "plan.review.authorize-reporter",
                "plan.task.update",
                "plan.task.attach",
                "plan.task.close",
                "plan.task.assign",
                "plan.task.dispatch",
                "plan.task.attempts",
                "plan.task.attempt.show",
                "plan.task.respawn",
                "plan.task.adopt_checkpoint",
                "plan.task.compete",
                "plan.task.judge",
                "plan.task.rm",
                "plan.lane_remove",
                "plan.milestone.add",
                "plan.milestone.update",
                "plan.milestone.rm",
                "plan.stride.add",
                "plan.skills.list",
                "plan.skills.show",
                "plan.skills.new",
                "plan.retrospective",
                "plan.loop.create",
                "plan.loop.set",
                "plan.loop.status",
                "plan.loop.stop",
                "plan.loop.record",
                "plan.layout.set",
                "plan.note.add",
                "plan.note.update",
                "plan.element.add",
                "plan.element.update",
                "plan.element.rm",
                "plan.element.list",
                "plan.element.show",
                "spec.create",
                "spec.list",
                "spec.show",
                "spec.files",
                "spec.section.add",
                "spec.section.update",
                "spec.section.rm",
                "spec.link.add",
                "spec.link.rm",
                "spec.link.list",
                "effort.show",
                "effort.link",
                "task.list",
                "task.show",
                "task.take",
                "task.drop",
                "task.add",
                "canvas.focus",
                "canvas.filter",
                "cron.list",
                "cron.get",
                "cron.create",
                "cron.update",
                "cron.delete",
                "cron.enable",
                "cron.disable",
                "cron.trigger",
                "cron.cancel",
                "cron.history",
                "cron.complete",
                "services.list",
                "services.create",
                "services.get",
                "services.update",
                "services.delete",
                "services.regenerate_token",
                "activity.list",
                "bridge.send",
                "bridge.react",
                "bridge.restart",
                "mem.stats",
                "mem.search",
                "mem.get",
                "mem.save",
                "mem.delete",
                "mem.forget",
                "mem.update",
                "mem.pinned",
                "mem.append",
                "mem.restore",
                "mem.wake_up",
                "mem.learn",
                "mem.learn_promote",
                "mem.flush",
                "mem.sync",
                "mem.reembed",
                "mem.extract_pending",
                "mem.history",
                "mem.taxonomy",
                "mem.tail",
                "mem.calibration.log",
                "mem.calibration.sweep",
                "mem.worker.spawn",
                "mem.worker.list",
                "mem.worker.get",
                "mem.worker.progress",
                "mem.worker.report",
                "mem.worker.kill",
                "mem.graph.create",
                "mem.graph.relate",
                "mem.graph.related",
                "mem.graph.invalidate",
                "mem.graph.timeline",
                "mem.graph.traverse",
                "mem.graph.path",
                "mem.graph.tunnels",
                "mem.graph.alias",
                "mem.graph.repair.find_relations",
                "mem.graph.repair.delete_relations",
                "mem.graph.repair.merge_relations",
                "mem.graph.repair.find_entities",
                "mem.graph.repair.augment_entity",
                "mem.graph.repair.delete_entity",
                "mem.graph.repair.orphan_vectors",
                "mem.graph.repair.prune_thin_vectors",
                "mem.graph.repair.collapse_synonym_edges",
                "mem.graph.repair.prune_dream_hub_edges",
                "mem.graph.repair.orphan_blobs",
                // Credential-bearing mem.secret.* stays on bearer-authenticated HTTP.
            ];
            return Ok(serde_json::json!({ "methods": methods }));
        }
        "mesh.status" => {
            let mesh = app.state::<crate::MeshStateHandle>();
            let manager = mesh.lock();
            return serde_json::to_value(manager.status()).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            });
        }
        "mesh.peers" => {
            let mesh = app.state::<crate::MeshStateHandle>();
            let mut manager = mesh.lock();
            manager.cleanup_expired_requests();
            return serde_json::to_value(manager.peers_response()).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            });
        }
        #[cfg(debug_assertions)]
        "debug.crash" => {
            let kind = get_string_param(&params, "kind")?;
            crate::crash::trigger_test_crash(&kind).map_err(|e| JsonRpcError {
                code: -32602,
                message: e,
                data: None,
            })?;
            return Ok(serde_json::json!({"ok": true, "kind": kind}));
        }
        _ => {}
    }

    let prefix = method.split(['.', ':']).next().unwrap_or("");
    if params
        .as_ref()
        .is_some_and(|value| value.get("task_id").is_some())
        && matches!(
            method,
            "task.take"
                | "task.drop"
                | "plan.task.update"
                | "plan.task.attach"
                | "plan.task.close"
                | "plan.task.pause"
                | "plan.task.resume"
                | "plan.task.assign"
                | "plan.task.dispatch"
                | "plan.task.respawn"
                | "plan.task.compete"
                | "plan.task.judge"
                | "plan.task.rm"
                | "plan.task.link"
        )
    {
        plan_task_authority::verify_owner_epoch_for_task_mutation(method, &params, svc)?;
    }
    match prefix {
        "workspace" => workspace::dispatch(method, &params, mux, app, svc),
        "team" => team::dispatch(method, &params, mux, svc),
        "tab" | "agents" => tab::dispatch(method, &params, mux, app, svc),
        "notification" => notification::dispatch(method, &params, svc),
        "agent" => agent::dispatch(method, &params, app, svc),
        "orchestration" => orchestration::dispatch(method, &params, app, svc),
        "orchestrator" => orchestrator::dispatch(method, &params, app, svc),
        "persona" => persona::dispatch(method, &params),
        "plan" if plan_mode::handles(method) => plan_mode::dispatch(method, &params, app, svc),
        "plan" | "task" => plan::dispatch(method, &params, app, svc),
        "canvas" => canvas::dispatch(method, &params, app),
        "spec" | "effort" => spec::dispatch(method, &params, app, svc),
        "plugin" => agent_commands::dispatch(method, &params, svc),
        "cron" => cron::dispatch(method, &params, app, svc),
        "services" | "activity" => services::dispatch(method, &params, app, svc),
        "bridge" => bridge::dispatch(method, &params, svc),
        "mem" => mem::dispatch(method, params, svc),
        _ => Err(JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        }),
    }
}

pub(crate) fn dispatch_service_rpc(
    method: &str,
    params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    dispatch(
        method,
        params,
        &svc.mux,
        &svc.pty,
        &svc.notif,
        &svc.app_handle,
        &svc.output_buf,
        svc,
    )
}
