//! `AgentTransport` — the runtime half of a provider behind one seam, plus the
//! atomic spawn entry the frontend calls. Both impls wrap existing substrates
//! ([`PtyTransport`] → the FE-delivered launch command; [`ChatTransport`] →
//! `chatbackend_launch_async`). [`ServiceContext::spawn_pane_atomic`] builds the
//! pane and kicks launch async, returning a snapshot the client applies once.

use serde::{Deserialize, Serialize};
use tauri::Manager;

use super::spawn::{should_default_agent_pane_to_chat, PaneVisibility};
use super::ServiceContext;
use crate::agent::provider::{AgentLaunchParams, ProviderTransport};
use crate::multiplexer::state::MultiplexerSnapshot;
use crate::multiplexer::types::PaneBacking;

/// Frontend-facing params for the one atomic spawn command.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtomicSpawnParams {
    pub agent_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_true")]
    pub skip_permissions: bool,
    /// chatBackend openSession config (systemPrompt/preset/model). Ignored for PTY.
    #[serde(default)]
    pub config: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// Authoritative result the client applies once. `launch_command` is `Some` only
/// for PTY (the FE types it when ready); chat opens its session in the background, reporting `None`.
#[derive(Debug, Clone, Serialize)]
pub struct AtomicSpawn {
    pub snapshot: MultiplexerSnapshot,
    pub pane_id: String,
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
    pub pane_id: &'a str,
    pub tab_id: &'a str,
    pub workspace_id: &'a str,
    pub skip_permissions: bool,
    pub config: serde_json::Value,
}

/// The runtime half of a provider. `launch` never blocks pane visibility: it
/// either returns the command the FE delivers (PTY) or kicks the network
/// openSession onto a background task and returns `None` (chat).
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

pub struct PtyTransport;
pub struct ChatTransport;

impl AgentTransport for PtyTransport {
    fn launch(
        &self,
        svc: &ServiceContext,
        ctx: &TransportLaunchCtx<'_>,
    ) -> Result<Option<String>, String> {
        let model_args = crate::service::spawn::resolve_fresh_agent_model(
            &svc.agent_registry,
            ctx.provider_id,
            None,
            None,
        )
        .and_then(|model| {
            svc.agent_registry
                .with_provider(ctx.provider_id, |p| p.model_launch_args(&model))
        })
        .unwrap_or_default();
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: (!model_args.is_empty()).then_some(model_args),
            skip_permissions: ctx.skip_permissions,
            session_id: None,
            structured_prompt_server: None,
        };
        svc.agent_registry
            .with_provider(ctx.provider_id, |p| p.build_launch_command(&params))
            .map(Some)
            .ok_or_else(|| format!("Unknown agent provider: {}", ctx.provider_id))
    }
}

impl AgentTransport for ChatTransport {
    fn launch(
        &self,
        svc: &ServiceContext,
        ctx: &TransportLaunchCtx<'_>,
    ) -> Result<Option<String>, String> {
        let config = if ctx.config.is_null() {
            serde_json::Value::Object(Default::default())
        } else {
            ctx.config.clone()
        };
        // Stamp the connecting backing synchronously so the pane renders as chat at once, not briefly as a terminal with a stray PTY.
        {
            let mut mux = svc.mux.write();
            if let Some(pane) = mux.get_pane_mut(ctx.pane_id) {
                pane.backing = PaneBacking::ChatBackend {
                    plugin_id: ctx.provider_id.to_string(),
                    session_id: String::new(),
                };
                pane.chat_view = true;
            }
        }
        let history_meta = config.clone();
        svc.chatbackend_launch_async(
            ctx.provider_id.to_string(),
            ctx.pane_id.to_string(),
            ctx.tab_id.to_string(),
            ctx.workspace_id.to_string(),
            config,
            history_meta,
        );
        Ok(None)
    }
}

impl ServiceContext {
    /// The atomic spawn entry: pick the transport, build the pane synchronously, kick launch async, return the snapshot.
    pub fn spawn_pane_atomic(&self, params: AtomicSpawnParams) -> Result<AtomicSpawn, String> {
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
                self.spawn_pane_atomic_with(&provider, &params, &ChatTransport, true, None)
            }
            Some(ProviderTransport::Pty) => {
                self.require_agent_provider_ready(&provider)?;
                let chat_view = self.default_agent_chat_view(&provider);
                self.spawn_pane_atomic_with(
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
                self.spawn_pane_atomic_with(
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
        let default_enabled = self
            .app_handle
            .state::<crate::config::ConfigState>()
            .lock()
            .ui
            .default_new_agent_pane_chat;
        should_default_agent_pane_to_chat(
            default_enabled,
            PaneVisibility::Visible,
            self.agent_registry
                .with_provider(provider, |p| p.supports_chat_ui())
                .unwrap_or(false),
        )
    }

    /// Create + cwd + chat_view + activate + focus, then launch. `agent_type` is set
    /// only for PTY providers; chat panes carry identity in their `backing`.
    fn spawn_pane_atomic_with(
        &self,
        provider: &str,
        params: &AtomicSpawnParams,
        transport: &dyn AgentTransport,
        chat_view: bool,
        agent_type: Option<&str>,
    ) -> Result<AtomicSpawn, String> {
        let (ws_id, ws_cwd) = self.resolve_spawn_workspace(params.workspace_id.clone())?;
        let (resolved_cwd, warning) = Self::resolve_agent_cwd(&params.cwd, &ws_cwd);

        let (tab_id, pane_id) = {
            let mut mux = self.mux.write();
            let (tab_id, pane_id) = mux.create_tab(&ws_id)?;
            if let Some(pane) = mux.get_pane_mut(&pane_id) {
                if let Some(ref dir) = resolved_cwd {
                    pane.cwd = Some(dir.clone());
                }
                if chat_view {
                    pane.chat_view = true;
                }
            }
            let _ = mux.set_active_workspace(&ws_id);
            let _ = mux.set_active_tab(&ws_id, &tab_id);
            (tab_id, pane_id)
        };

        if let Some(at) = agent_type {
            // PTY agent tabs default to Orchestrator (can spawn); chat panes keep their default (identity in `backing`).
            let _ = self.set_pane_role(&pane_id, "Orchestrator", None, Some(true));
            self.set_pane_agent_type(&pane_id, Some(at.to_string()));
        }
        let _ = self.focus_pane(&pane_id);

        // The tab is already created/activated/focused; roll it back on launch failure so the user isn't left on a blank tab.
        let launch_command = finish_launch(
            transport.launch(
                self,
                &TransportLaunchCtx {
                    provider_id: provider,
                    pane_id: &pane_id,
                    tab_id: &tab_id,
                    workspace_id: &ws_id,
                    skip_permissions: params.skip_permissions,
                    config: params.config.clone().unwrap_or(serde_json::Value::Null),
                },
            ),
            || {
                self.mux
                    .write()
                    .rollback_spawn_tab(&ws_id, &tab_id, &pane_id);
                self.emit_state_changed();
            },
        )?;

        let snapshot = self.mux.read().snapshot();
        self.emit_state_changed();

        Ok(AtomicSpawn {
            snapshot,
            pane_id,
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
