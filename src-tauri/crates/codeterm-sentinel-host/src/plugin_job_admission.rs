use crate::{pipeline::plugin, scheduler::Handle};
use codeterm_plugin::{
    blueprint_step::*,
    plugin_jobs::PluginJobAdmission,
    view_host::{current_view_call_context, ViewRegistryHandle},
};
use codeterm_service_core::service::ServiceContext;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Weak;

pub(crate) struct JobAdmission(pub Weak<ServiceContext>);

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Operations,
    Admit,
    Track { operation: String, job: String },
    Status { operation: String },
    Check { operation: String },
    Cancel { operation: String },
    Stop { operation: String },
}

fn origin(
    service: &ServiceContext,
    plugin: &str,
) -> Result<(PluginJobScope, PluginJobOrigin), String> {
    if let Some(context) = codeterm_plugin::surface_invocation::current(plugin) {
        if service
            .ext::<ViewRegistryHandle>()
            .registration_id(plugin)
            .as_deref()
            != Some(context.registration.as_str())
        {
            return Err("surface provider registration changed".into());
        }
        let mux = service.mux.read();
        let tab = mux
            .get_tab(&context.tab)
            .ok_or("surface instance unavailable")?;
        let backing = tab.surface().ok_or("surface instance unavailable")?;
        let owner = mux
            .get_tab(&context.owner)
            .ok_or("surface owner unavailable")?;
        let actor = mux
            .get_tab(&context.actor)
            .ok_or("surface actor unavailable")?;
        if backing.plugin_id != plugin
            || backing.surface_id != context.surface
            || backing.instance_id != context.instance
            || tab.parent_tab_id.as_deref() != Some(context.owner.as_str())
            || owner.team_id != context.team
            || actor.team_id != context.team
            || [&context.tab, &context.owner, &context.actor]
                .into_iter()
                .any(|tab| {
                    mux.find_workspace_for_tab(tab).as_deref() != Some(context.workspace.as_str())
                })
        {
            return Err("surface invocation scope changed".into());
        }
        return Ok((
            PluginJobScope {
                plugin: plugin.into(),
                workspace: context.workspace,
                team: context.team,
                owner: context.owner,
            },
            PluginJobOrigin::StudioAgent {
                surface: context.surface,
                instance: context.instance,
                tab: context.tab,
                actor: context.actor,
            },
        ));
    }
    let context =
        current_view_call_context(plugin).ok_or("registered document context required")?;
    service.validate_plugin_view_context(&context)?;
    let scope = PluginJobScope {
        plugin: plugin.into(),
        workspace: context
            .workspace
            .clone()
            .ok_or("owned document workspace required")?,
        owner: context.owner.clone().ok_or("owned document required")?,
        team: context.team.clone(),
    };
    let origin = PluginJobOrigin::StudioView {
        surface: context.surface.clone().ok_or("surface required")?,
        instance: context.instance.clone().ok_or("instance required")?,
        tab: context.tab.clone().ok_or("surface tab required")?,
        frame: context.frame.clone().ok_or("document required")?,
    };
    Ok((scope, origin))
}

impl PluginJobAdmission for JobAdmission {
    fn invoke(&self, plugin_id: &str, request: &str) -> Result<Value, String> {
        let request: Request =
            serde_json::from_str(request).map_err(|_| "invalid closed plugin job request")?;
        let service = self.0.upgrade().ok_or("plugin job service unavailable")?;
        let handle = service
            .host_extension::<Handle>()
            .ok_or("Blueprint scheduler unavailable")?;
        let now = handle.store.lock().clock.now_ms();
        let ledger = plugin::ledger(&service)?;
        if matches!(&request, Request::Operations) {
            let (scope, _) = origin(&service, plugin_id)?;
            return serde_json::to_value(ledger.owner_operations(&scope)?)
                .map_err(|_| "plugin history unavailable".into());
        }
        if matches!(&request, Request::Admit) {
            let registry = service.ext::<ViewRegistryHandle>();
            let provider = registry.get(plugin_id).ok_or("plugin unavailable")?;
            if provider
                .job_lifecycle()
                .is_none_or(|descriptor| !descriptor.valid())
            {
                return Err("plugin job lifecycle unavailable".into());
            }
            let (scope, projection) = origin(&service, plugin_id)?;
            let receipt = ledger.admit_job_origin(scope, projection, now)?;
            let _ = origin(&service, plugin_id)?;
            return Ok(
                json!({"operation":receipt.operation,"deadlineMs":receipt.deadline_ms,"authorization":"observation_only"}),
            );
        }
        let operation = match &request {
            Request::Track { operation, .. }
            | Request::Status { operation }
            | Request::Check { operation }
            | Request::Cancel { operation }
            | Request::Stop { operation } => operation,
            Request::Admit | Request::Operations => unreachable!(),
        };
        if operation.len() != 64 || !operation.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid plugin receipt".into());
        }
        let receipt = ledger.job_origin(plugin_id, operation)?;
        if let Request::Track { job, .. } = &request {
            let receipt = ledger.attach_existing_job(plugin_id, operation, job)?;
            let available = service
                .ext::<ViewRegistryHandle>()
                .get(plugin_id)
                .is_some_and(|provider| {
                    provider
                        .job_lifecycle()
                        .is_some_and(|descriptor| descriptor.valid())
                });
            if receipt.deadline_ms <= now || !available {
                return Ok(
                    json!({"operation":operation,"job":job,"registered":false,"state":"unknown","reason":if available { "deadline" } else { "plugin_unavailable" }}),
                );
            }
            let result = if let Some(existing) = ledger.job_monitor(operation)? {
                Ok(existing)
            } else {
                ledger.monitor_existing_job(
                    PluginJobRegistration {
                        scope: receipt.scope,
                        origin: receipt.origin,
                        operation: operation.clone(),
                        job: job.clone(),
                        next_check_ms: now.saturating_add(1000),
                        deadline_ms: receipt.deadline_ms,
                    },
                    now,
                )
            };
            handle.notify();
            return Ok(match result {
                Ok(monitor) => {
                    json!({"operation":operation,"job":job,"registered":true,"monitor":monitor.public_view()?})
                }
                Err(_) => {
                    json!({"operation":operation,"job":job,"registered":false,"state":"unknown","reason":"monitor_unavailable"})
                }
            });
        }
        let (scope, _) = origin(&service, plugin_id)?;
        if receipt.scope != scope {
            return Err("plugin receipt owner scope refused".into());
        }
        if matches!(&request, Request::Check { .. }) && ledger.job_monitor(operation)?.is_none() {
            let job = receipt
                .job
                .as_ref()
                .ok_or("plugin job identity unavailable")?;
            let available = service
                .ext::<ViewRegistryHandle>()
                .get(plugin_id)
                .is_some_and(|provider| {
                    provider
                        .job_lifecycle()
                        .is_some_and(|descriptor| descriptor.valid())
                });
            if !available {
                return Err("plugin job lifecycle unavailable".into());
            }
            ledger.monitor_existing_job(
                PluginJobRegistration {
                    scope: scope.clone(),
                    origin: receipt.origin.clone(),
                    operation: operation.clone(),
                    job: job.clone(),
                    next_check_ms: now.saturating_add(1000),
                    deadline_ms: receipt.deadline_ms,
                },
                now,
            )?;
        }
        match &request {
            Request::Status { .. } => (),
            Request::Check { .. } => ledger.wake_job_monitor(operation, &scope, now)?,
            Request::Cancel { .. } => {
                ledger.request_job_cancel(operation, &scope)?;
                ledger.wake_job_monitor(operation, &scope, now)?;
            }
            Request::Stop { .. } => {
                ledger.stop_job_monitor(operation, &scope, MonitorStop::OwnerStopped)?
            }
            _ => unreachable!(),
        }
        handle.notify();
        Ok(
            json!({"operation":operation,"job":receipt.job,"monitor":ledger.job_monitor(operation)?.map(|monitor| monitor.public_view()).transpose()?}),
        )
    }
}
