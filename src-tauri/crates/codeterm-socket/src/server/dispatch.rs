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
    let canonical_method = method
        .strip_prefix("sentinel.")
        .map(|suffix| format!("blueprint.{suffix}"));
    let method = canonical_method.as_deref().unwrap_or(method);
    // System methods are tiny — handle inline.
    match method {
        "plugin.refresh_installed" => {
            let ids: Vec<String> = params
                .as_ref()
                .and_then(|p| p.get("ids"))
                .cloned()
                .ok_or_else(|| JsonRpcError {
                    code: -32602,
                    message: "ids is required".into(),
                    data: None,
                })
                .and_then(|value| {
                    serde_json::from_value(value).map_err(|error| JsonRpcError {
                        code: -32602,
                        message: error.to_string(),
                        data: None,
                    })
                })?;
            codeterm_service_core::plugin::js_runtime::hot_reload::refresh_installed_plugins(&ids)
                .map_err(|message| JsonRpcError {
                    code: -32603,
                    message,
                    data: None,
                })?;
            return Ok(serde_json::json!({"registered": ids}));
        }
        "plugin.core_guides" => {
            return Ok(
                codeterm_service_core::service::plugin_prompt::core_plugin_guides(
                    svc.ext::<codeterm_service_core::agent_commands::AgentCommandRegistryHandle>(),
                ),
            )
        }
        "rest_api.status" | "rest_api.restart" => {
            let control = app
                .state::<codeterm_service_core::startup::RestApiControlState>()
                .inner()
                .clone();
            return tauri::async_runtime::block_on(rest_api::execute(method, &control));
        }
        "api.request" => return local_api::dispatch(&params, app),
        "system.identify" => {
            return Ok(serde_json::json!({
                "name": "CodeTerm",
                "version": codeterm_build_info::version(),
                "platform": std::env::consts::OS,
                "features": ["multiplexer", "agent-detection", "notifications"]
            }));
        }
        "system.capabilities" => {
            let methods = vec![
                "system.identify",
                "plugin.refresh_installed",
                "system.capabilities",
                "rest_api.status",
                "rest_api.restart",
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
                "model_provider.add",
                "model_provider.add_from_secret",
                "model_provider.remove",
                "model_provider.list",
                "model_provider.test",
                "model_provider.preset.add",
                "model_provider.preset.remove",
                "model_provider.preset.list",
                "agent.list",
                "agent.hierarchy",
                "agent.spawn",
                "agent.diagnostic",
                "agent.set_role",
                "agent.clear_role",
                "agent.status",
                "agent.turns",
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
                "plan.task.split",
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
                "plan.lanes_clean",
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
                "blueprint.create",
                "blueprint.export",
                "blueprint.import",
                "blueprint.input",
                "blueprint.pipeline.create",
                "blueprint.pipeline.validate",
                "blueprint.pipeline.validate.update",
                "blueprint.pipeline.update",
                "blueprint.pipeline.nodes",
                "blueprint.log",
                "blueprint.validate.create",
                "blueprint.list",
                "blueprint.show",
                "blueprint.stop",
                "blueprint.delete",
                "blueprint.update",
                "blueprint.validate.update",
                "blueprint.history",
                "blueprint.escalation.resolve",
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
                "bridge.thread.open",
                "bridge.react",
                "bridge.restart",
                "mem.stats",
                "mem.search",
                "mem.get",
                "mem.save",
                "mem.delete",
                "mem.purge_precompact",
                "mem.forget",
                "mem.update",
                "mem.pinned",
                "mem.append",
                "mem.restore",
                "mem.merge",
                "mem.area_merge",
                "mem.wake_up",
                "mem.learn",
                "mem.learn_promote",
                "mem.sync",
                "mem.reembed",
                "mem.extract_pending",
                "mem.history",
                "mem.taxonomy",
                "mem.tail",
                "mem.calibration.log",
                "mem.calibration.sweep",
                "mem.calibration.stale",
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
            let plugins = codeterm_service_core::service::plugin_skills::granted_catalogs(
                svc.ext::<codeterm_service_core::agent_commands::AgentCommandRegistryHandle>(),
            );
            return Ok(serde_json::json!({ "methods": methods, "plugins": plugins }));
        }
        "mesh.status" => {
            let mesh = app.state::<codeterm_service_core::MeshStateHandle>();
            let manager = mesh.lock();
            return serde_json::to_value(manager.status()).map_err(|error| JsonRpcError {
                code: -32603,
                message: error.to_string(),
                data: None,
            });
        }
        "mesh.peers" => {
            let mesh = app.state::<codeterm_service_core::MeshStateHandle>();
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
            codeterm_service_core::crash::trigger_test_crash(&kind).map_err(|e| JsonRpcError {
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
    if matches!(
        method,
        "plan.archive"
            | "plan.unarchive"
            | "plan.delete"
            | "plan.disown"
            | "plan.claim"
            | "plan.update"
    ) {
        let plan_id = plan_refs::resolve_plan(&svc.plan_store, &params)?;
        plan_mode::authorize_plan_mutation(svc, &plan_id, &params)?;
    }
    match prefix {
        "workspace" => workspace::dispatch(method, &params, mux, app, svc),
        "team" => team::dispatch(method, &params, mux, svc),
        "tab" | "agents" => tab::dispatch(method, &params, mux, app, svc),
        "notification" => notification::dispatch(method, &params, svc),
        "agent" => agent::dispatch(method, &params, app, svc),
        "model_provider" => model_providers::dispatch(method, params, svc),
        "orchestration" => orchestration::dispatch(method, &params, app, svc),
        "orchestrator" => orchestrator::dispatch(method, &params, app, svc),
        "persona" => persona::dispatch(method, &params),
        "plan" if plan_mode::handles(method) => plan_mode::dispatch(method, &params, app, svc),
        "plan" | "task" => plan::dispatch(method, &params, app, svc),
        "canvas" => canvas::dispatch(method, &params, app),
        "spec" | "effort" => spec::dispatch(method, &params, app, svc),
        "plugin" if receiver_target::handles(method) => {
            receiver_target::dispatch(method, &params, svc)
        }
        "plugin" => agent_commands::dispatch(method, &params, svc),
        "blueprint" => blueprint::dispatch(method, &params, svc),
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

pub(super) fn dispatch_with_post_response(
    method: &str,
    params: Option<serde_json::Value>,
    mux: &MuxState,
    pty: &PtyState,
    notif: &NotifState,
    app: &AppHandle,
    output_buf: &OutputBufferState,
    svc: &ServiceState,
) -> Result<(serde_json::Value, Option<super::PostResponseAction>), JsonRpcError> {
    if method == "cron.complete" {
        return cron::dispatch_complete(&params, app, svc);
    }
    dispatch(method, params, mux, pty, notif, app, output_buf, svc).map(|result| (result, None))
}

pub(crate) fn dispatch_service_rpc(
    method: &str,
    mut params: Option<serde_json::Value>,
    svc: &ServiceState,
) -> Result<serde_json::Value, JsonRpcError> {
    if let Some(object) = params.as_mut().and_then(serde_json::Value::as_object_mut) {
        object.remove("authenticated_mesh_peer_id");
    }
    dispatch_service_rpc_inner(method, params, svc)
}

pub(crate) fn dispatch_mesh_rpc(
    method: &str,
    mut params: Option<serde_json::Value>,
    svc: &ServiceState,
    peer_id: &str,
) -> Result<serde_json::Value, JsonRpcError> {
    if let Some(object) = params.as_mut().and_then(serde_json::Value::as_object_mut) {
        object.insert(
            "authenticated_mesh_peer_id".into(),
            serde_json::Value::String(peer_id.into()),
        );
    }
    if method.starts_with("model_provider.") || method.starts_with("rest_api.") {
        return Err(JsonRpcError {
            code: -32600,
            message: if method.starts_with("rest_api.") {
                "REST listener operations are local-only".into()
            } else {
                "model provider operations are local-only".into()
            },
            data: None,
        });
    }
    dispatch_service_rpc_inner(method, params, svc)
}

fn dispatch_service_rpc_inner(
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

#[cfg(test)]
mod tests {
    use super::dispatch_service_rpc;
    use codeterm_service_core::cron::store::{
        CronCreateParams, CronRunInfo, CronRunStatus, CronStore,
    };
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use std::sync::Arc;
    use tauri::Manager;

    #[test]
    fn cron_settle_service_rpc_closes_completed_tab() {
        let (mut mux, initial_tab) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&initial_tab).unwrap();
        let tab_id = mux.create_tab(&workspace).unwrap();
        let fixture = ServiceFixture::new(mux);
        let mut store = CronStore::new_with_path(fixture.temp.path().join("crons.json"));
        let job_id = store
            .create(CronCreateParams {
                name: "rpc completion".into(),
                workspace_id: "workspace".into(),
                team_id: None,
                detach_from_team: false,
                schedule: "0 * * * *".into(),
                task: "complete run".into(),
                description: None,
                agent: None,
                model: None,
                reasoning_effort: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
                enabled: Some(false),
                max_history: None,
                oneshot: Some(false),
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: None,
            })
            .unwrap()
            .id;
        store
            .record_run(
                &job_id,
                CronRunInfo {
                    run_id: "service-rpc-complete-run".into(),
                    started_at: 1,
                    finished_at: None,
                    status: CronRunStatus::Running,
                    tab_id: Some(tab_id.clone()),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                },
            )
            .unwrap();
        store.register_tab(&tab_id, &job_id, "service-rpc-complete-run");
        let cron: codeterm_service_core::CronState = Arc::new(parking_lot::Mutex::new(store));
        fixture._app.manage(cron);
        fixture
            ._app
            .manage::<codeterm_service_core::CronWake>(Arc::new(std::sync::Condvar::new()));

        let response = dispatch_service_rpc(
            "cron.complete",
            Some(serde_json::json!({
                "run_id": "service-rpc-complete-run",
                "summary": "service-rpc-summary",
                "human_confirmed": true
            })),
            &Arc::new(fixture.service.clone()),
        )
        .unwrap();

        assert_eq!(response["ok"], true);
        assert!(fixture.service.mux.read().get_tab(&tab_id).is_none());
    }
}
