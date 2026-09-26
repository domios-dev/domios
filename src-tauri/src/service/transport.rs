//! `AgentTransport` — the runtime half of a provider behind one seam, plus the
//! atomic spawn entry the frontend calls. Both impls wrap existing substrates
//! ([`PtyTransport`] → backend-owned PTY launch; [`ChatTransport`] →
//! `chatbackend_launch`). [`ServiceContext::spawn_tab_atomic`] builds the tab
//! and returns a snapshot only after transport launch succeeds.

use serde::{Deserialize, Serialize};

use super::spawn::{should_default_agent_tab_to_chat, TabVisibility};
use super::ServiceContext;
use crate::agent::provider::{AgentLaunchParams, ProviderTransport};
use crate::multiplexer::state::MultiplexerSnapshot;
use crate::multiplexer::types::TabBacking;

/// Frontend-facing params for the one atomic spawn command.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicSpawnParams {
    pub agent_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// chatBackend openSession config (systemPrompt/preset/model). Ignored for PTY.
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    /// Explicit model for the spawned agent (PTY launch args / chat session model).
    #[serde(default)]
    pub model: Option<String>,
    /// Explicitly requested reasoning effort. Capability-gated against the
    /// provider: an unsupported value contributes no launch flag.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// Authoritative result the client applies once. PTY and chat launch work stays
/// in the backend, so no mounted frontend surface is required to start either.
#[derive(Debug, Clone, Serialize)]
pub struct AtomicSpawn {
    pub snapshot: MultiplexerSnapshot,
    pub tab_id: String,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Per-spawn context handed to a transport's `launch`.
pub struct TransportLaunchCtx<'a> {
    pub provider_id: &'a str,
    pub tab_id: &'a str,
    pub skip_permissions: bool,
    pub config: serde_json::Value,
    pub model: Option<&'a str>,
    pub reasoning_effort: Option<&'a str>,
}

/// The runtime half of a provider. Callers put blocking transports on a
/// blocking worker before invoking `launch`.
pub trait AgentTransport {
    fn launch(
        &self,
        svc: &ServiceContext,
        ctx: &TransportLaunchCtx<'_>,
    ) -> Result<Option<String>, String>;
}

/// On a launch `Err`, run `rollback` (close the half-built tab) then propagate;
/// on `Ok`, pass the command through. Factored out for unit-testing the unwind.
fn finish_launch<F: FnOnce()>(
    launch: Result<Option<String>, String>,
    rollback: F,
) -> Result<Option<String>, String> {
    launch.inspect_err(|_| rollback())
}

fn backend_owned_pty_launch(
    launch_command: String,
    dispatch: impl FnOnce(String),
) -> Option<String> {
    dispatch(launch_command);
    None
}

pub struct PtyTransport;
pub struct ChatTransport;

impl AgentTransport for PtyTransport {
    fn launch(
        &self,
        svc: &ServiceContext,
        ctx: &TransportLaunchCtx<'_>,
    ) -> Result<Option<String>, String> {
        let model_resolution = crate::service::spawn::resolve_fresh_agent_model_with_notice(
            &svc.agent_registry,
            ctx.provider_id,
            ctx.model.map(str::to_string),
            None,
        );
        if let Some(tab) = svc.mux.write().get_tab_mut(ctx.tab_id) {
            crate::service::spawn::persist_model_launch_identity(tab, None, &model_resolution);
        }
        let model_args = model_resolution
            .model
            .as_deref()
            .and_then(|model| {
                svc.agent_registry
                    .with_provider(ctx.provider_id, |p| p.model_launch_args(model))
            })
            .unwrap_or_default();
        let mut args = model_args;
        if let Some(effort) = ctx.reasoning_effort.filter(|e| !e.trim().is_empty()) {
            if let Some(effort_args) = svc
                .agent_registry
                .with_provider(ctx.provider_id, |p| {
                    p.supports_reasoning_effort_for_model(model_resolution.model.as_deref(), effort)
                        .then(|| p.reasoning_effort_launch_args(effort))
                })
                .flatten()
            {
                args.extend(effort_args);
            }
        }
        let cwd = svc
            .mux
            .read()
            .get_tab(ctx.tab_id)
            .and_then(|p| p.cwd.clone())
            .unwrap_or_default();
        let structured_prompt_server = crate::service::structured_prompt::ensure_for_spawn(
            svc,
            ctx.provider_id,
            ctx.tab_id,
            &cwd,
        );
        let (role, role_profile) = svc
            .mux
            .read()
            .get_tab(ctx.tab_id)
            .map(|tab| {
                (
                    tab.role.map(|role| format!("{role:?}")),
                    tab.role_profile.clone(),
                )
            })
            .unwrap_or_default();
        let launch_task = crate::service::orchestrator::provider_launch_task(
            &svc.agent_registry,
            ctx.provider_id,
            None,
            role.as_deref(),
            role_profile.as_deref(),
        );
        let mut params = AgentLaunchParams {
            agent_name: None,
            task: launch_task.clone(),
            starter_prompt: crate::agent::provider::StarterPrompt::NoStarter,
            args: (!args.is_empty()).then_some(args),
            skip_permissions: ctx.skip_permissions,
            session_id: None,
            structured_prompt_server,
            launch_marker: None,
        };
        let launch_command =
            crate::service::launch_session_detection::marked_provider_launch_command(
                &svc.agent_registry,
                ctx.provider_id,
                ctx.tab_id,
                &mut params,
            )
            .ok_or_else(|| format!("Unknown agent provider: {}", ctx.provider_id))?;
        Ok(backend_owned_pty_launch(launch_command, |launch_command| {
            crate::service::orchestrator::launch_agent_in_background(
                svc.clone(),
                ctx.tab_id.to_string(),
                launch_command,
                ctx.provider_id.to_string(),
                crate::service::launch_diagnostics::AgentLaunchOrigin::AtomicSpawn,
                launch_task,
                None,
                None,
                None,
            );
        }))
    }
}

impl AgentTransport for ChatTransport {
    fn launch(
        &self,
        svc: &ServiceContext,
        ctx: &TransportLaunchCtx<'_>,
    ) -> Result<Option<String>, String> {
        let mut config = if ctx.config.is_null() {
            serde_json::Value::Object(Default::default())
        } else {
            ctx.config.clone()
        };
        if let (Some(model), Some(obj)) = (ctx.model, config.as_object_mut()) {
            obj.entry("model")
                .or_insert_with(|| serde_json::Value::String(model.to_string()));
        }
        // Stamp the connecting backing synchronously so the tab renders as chat at once, not briefly as a terminal with a stray PTY.
        {
            let mut mux = svc.mux.write();
            if let Some(tab) = mux.get_tab_mut(ctx.tab_id) {
                tab.backing = TabBacking::ChatBackend {
                    plugin_id: ctx.provider_id.to_string(),
                    session_id: String::new(),
                };
                tab.chat_view = true;
            }
        }
        let history_meta = config.clone();
        svc.chatbackend_launch(
            ctx.provider_id.to_string(),
            ctx.tab_id.to_string(),
            config,
            history_meta,
        )?;
        Ok(None)
    }
}

impl ServiceContext {
    /// The atomic spawn entry: pick the transport, build the tab, launch it,
    /// then return the authoritative snapshot.
    pub fn spawn_tab_atomic(&self, params: AtomicSpawnParams) -> Result<AtomicSpawn, String> {
        let provider = if params.agent_id.trim().is_empty() {
            crate::config::default_agent_provider()
        } else {
            params.agent_id.clone()
        };

        match self.provider_registry.transport_of(&provider) {
            Some(ProviderTransport::ChatBackend) => {
                if !self.provider_registry.is_spawnable_chat_backend(&provider) {
                    return Err(format!(
                        "'{}' is a chat backend that is not user-spawnable",
                        provider
                    ));
                }
                self.spawn_tab_atomic_with(&provider, &params, &ChatTransport, true, None)
            }
            Some(ProviderTransport::Pty) => {
                self.require_agent_provider_ready(&provider)?;
                let chat_view = self.default_agent_chat_view(&provider);
                self.spawn_tab_atomic_with(
                    &provider,
                    &params,
                    &PtyTransport,
                    chat_view,
                    Some(&provider),
                )
            }
            None => {
                // Unknown id → honest readiness error (never a guess).
                self.require_agent_provider_ready(&provider)?;
                let chat_view = self.default_agent_chat_view(&provider);
                self.spawn_tab_atomic_with(
                    &provider,
                    &params,
                    &PtyTransport,
                    chat_view,
                    Some(&provider),
                )
            }
        }
    }

    fn default_agent_chat_view(&self, provider: &str) -> bool {
        should_default_agent_tab_to_chat(
            TabVisibility::Visible,
            self.agent_registry
                .with_provider(provider, |p| p.supports_chat_ui())
                .unwrap_or(false),
        )
    }

    /// Create + cwd + chat_view + activate + focus, then launch. `agent_type` is set
    /// only for PTY providers; chat tabs carry identity in their `backing`.
    fn spawn_tab_atomic_with(
        &self,
        provider: &str,
        params: &AtomicSpawnParams,
        transport: &dyn AgentTransport,
        chat_view: bool,
        agent_type: Option<&str>,
    ) -> Result<AtomicSpawn, String> {
        let (ws_id, ws_cwd) = self.resolve_spawn_workspace(params.workspace_id.clone())?;
        let (resolved_cwd, warning) = Self::resolve_agent_cwd(&params.cwd, &ws_cwd);

        let tab_id = {
            let mut mux = self.mux.write();
            let tab_id = mux.create_tab(&ws_id)?;
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
                tab.agent_requested_model = params
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|m| !m.is_empty())
                    .map(str::to_string);
                if chat_view {
                    tab.chat_view = true;
                }
            }
            let _ = mux.set_active_workspace(&ws_id);
            let _ = mux.set_active_tab(&ws_id, &tab_id);
            tab_id
        };

        let permission_bypass = crate::service::permission_policy::configured_permission_bypass(
            &self.agent_registry,
            provider,
        )?
        .is_some();
        if let Some(tab) = self.mux.write().get_tab_mut(&tab_id) {
            tab.auto_approve = permission_bypass;
        }

        if let Some(at) = agent_type {
            self.write_tab_agent_type_with_origin(
                &tab_id,
                at.to_string(),
                false,
                crate::session::lifecycle::AgentTypeWriteOrigin::Spawn,
                None,
            );
        }
        let pad = self.ensure_agent_pad(&tab_id).inspect_err(|_| {
            self.rollback_provisional_tab(&ws_id, &tab_id);
            self.emit_state_changed();
        })?;
        let _ = self.focus_tab(&tab_id);

        // The tab is already created/activated/focused; roll it back on launch failure so the user isn't left on a blank tab.
        let launch_command = finish_launch(
            transport.launch(
                self,
                &TransportLaunchCtx {
                    provider_id: provider,
                    tab_id: &tab_id,
                    skip_permissions: permission_bypass,
                    config: params.config.clone().unwrap_or(serde_json::Value::Null),
                    model: params.model.as_deref(),
                    reasoning_effort: params.reasoning_effort.as_deref(),
                },
            ),
            || {
                self.rollback_provisional_tab(&ws_id, &tab_id);
                let _ = self.revoke_agent_pad(&tab_id);
                self.emit_state_changed();
            },
        )?;
        drop(pad);

        let snapshot = self.mux.read().snapshot();
        self.emit_state_changed();

        Ok(AtomicSpawn {
            snapshot,
            tab_id,
            workspace_id: ws_id,
            launch_command,
            warning,
        })
    }

    /// Resolve the target workspace id + its cwd for a spawn.
    fn resolve_spawn_workspace(
        &self,
        requested: Option<String>,
    ) -> Result<(String, Option<String>), String> {
        let mux = self.mux.read();
        let id = requested
            .or_else(|| mux.get_active_workspace().map(|w| w.id.clone()))
            .or_else(|| mux.list_workspaces().first().map(|w| w.id.clone()))
            .ok_or("No workspace found")?;
        let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
        Ok((id, cwd))
    }
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
