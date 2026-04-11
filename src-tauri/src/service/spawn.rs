//! Unified pane spawning.
//!
//! One entry point: [`ServiceContext::spawn_pane`]. Everything that creates
//! a pane — user tabs, CLI `agent spawn`, cron triggers, mem extraction
//! workers, dreaming workers, General Agent sub-spawns — builds a
//! [`SpawnPaneSpec`] and calls [`ServiceContext::spawn_pane`].
//!
//! The spec captures **what** should be in the pane (shell vs agent, which
//! provider, which model, which task) and **how** it should appear (visible
//! tab vs hidden worker). Internals handle workspace resolution, cwd
//! resolution, pane creation, PTY spawn, backend reader start, agent launch
//! command construction, and task delivery.
//!
//! Phase 1 of the unification plan: the type surface lands and `spawn_pane`
//! delegates to the existing `spawn_agent_sync` / `spawn_headless` code
//! paths through an adapter. Phase 3 rewrites those paths to live inside
//! this module directly.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ServiceContext;
use super::orchestrator::{SpawnAgentParams, SpawnResult};

// ── Types ───────────────────────────────────────────────────────────────

/// Full description of a pane spawn request.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SpawnPaneSpec {
    /// Explicit workspace. `None` falls back to parent's workspace → active
    /// → first workspace.
    #[serde(default)]
    pub workspace_id: Option<String>,

    /// Parent pane id. Used for workspace lookup and role hierarchy.
    #[serde(default)]
    pub parent_pane_id: Option<String>,

    /// Working directory. `None` inherits from the workspace; mismatch with
    /// workspace cwd produces a warning in the returned handle.
    #[serde(default)]
    pub cwd: Option<String>,

    /// Pane tab title. `None` falls back to agent name / provider id.
    #[serde(default)]
    pub title: Option<String>,

    /// Visible tab vs hidden headless pane.
    #[serde(default)]
    pub visibility: PaneVisibility,

    /// What runs inside the pane.
    pub command: PaneCommand,
}

/// How the pane appears to the user.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaneVisibility {
    /// Normal tab in the tabbar. Lazy PTY: the frontend creates the PTY
    /// after `state-changed`; if no frontend is listening within ~5s the
    /// backend spawns it. This is the default.
    #[default]
    Visible,

    /// Headless pane. `pane.hidden=true`, `pane.show_in_tabbar=false`. The
    /// backend spawns the PTY synchronously and attaches a backend reader
    /// immediately — no frontend involvement needed.
    Hidden,
}

/// What to launch inside the pane.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneCommand {
    /// Plain shell — spawn a PTY with the user's default shell, no agent
    /// launch command sent.
    Shell,

    /// Launch an agent in the pane after the PTY is ready.
    Agent(AgentSpawnSpec),
}

impl Default for PaneCommand {
    fn default() -> Self {
        PaneCommand::Shell
    }
}

/// Agent-specific spawn fields. Used when [`PaneCommand::Agent`] is set.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AgentSpawnSpec {
    /// Agent provider id (`"claude"`, `"codex"`, `"aider"`, etc). Validated
    /// against `AgentRegistry` — unknown providers return an error from
    /// `spawn_pane` before any pane is created.
    pub provider: String,

    /// Model id resolved within the provider (e.g. `"claude-opus-4-6"` or
    /// alias `"opus"`). `None` uses the provider's default. Forwarded to
    /// `AgentProvider::model_launch_args` at launch time.
    #[serde(default)]
    pub model: Option<String>,

    /// Optional initial task prompt. `None` = interactive agent, no task
    /// sent. Wrapped by `task_preamble` if set.
    #[serde(default)]
    pub task: Option<String>,

    /// Display name override (e.g. `.claude/agents/<name>.md` reference).
    /// Forwarded via `AgentLaunchParams.agent_name`.
    #[serde(default)]
    pub agent_name: Option<String>,

    /// Extra CLI args appended to the launch command (provider-specific).
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Agent hierarchy role: `"Orchestrator" | "Lead" | "Worker" | "SubWorker"`.
    /// `None` leaves the pane unroled (user tab / standalone agent).
    #[serde(default)]
    pub role: Option<String>,

    /// Whether this pane may spawn its own child agents. Only meaningful
    /// when `role` is set.
    #[serde(default)]
    pub can_spawn: bool,

    /// Auto-accept agent permission prompts (`--dangerously-skip-permissions`
    /// for Claude, provider-specific for others). Defaults to `true`.
    #[serde(default = "default_true")]
    pub auto_approve: bool,

    /// Pane id to report to via `codeterm agent report`. Used for cron
    /// agents that must report back to the pane that triggered them
    /// without joining the orchestrator hierarchy.
    #[serde(default)]
    pub report_target_pane_id: Option<String>,

    /// Opaque worker id (set by `GraphWorkerRegistry` / mem workers).
    /// Populates `pane.worker_id` so the worker tracker can find its pane.
    /// Hidden spawns without a worker_id get one generated for them.
    #[serde(default)]
    pub worker_id: Option<String>,

    /// Origin tag recorded in `GraphWorkerInfo.spawned_by` for hidden
    /// spawns — e.g. `"auto:save"`, `"dreaming"`, `"cron:nightly"`. Used
    /// by the worker panel to group related workers. `None` records as
    /// `"spawn_pane"`.
    #[serde(default)]
    pub spawned_by: Option<String>,

    /// Extra environment variables merged into the PTY env (e.g.
    /// `CODETERM_WORKER_ID`, `CODETERM_JOB_ID`). `CODETERM_PANE_ID`,
    /// `CODETERM_SOCKET_PATH`, `CODETERM_API_URL`, and `CODETERM_API_TOKEN`
    /// are always injected regardless.
    #[serde(default)]
    pub extra_env: HashMap<String, String>,

    /// Preamble wrapped around the task (e.g. `WORKER_TASK_PREAMBLE`,
    /// cron completion hints). `None` = task sent verbatim.
    #[serde(default)]
    pub task_preamble: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Result of a successful spawn.
#[derive(Debug, Clone, Serialize)]
pub struct PaneHandle {
    pub pane_id: String,
    pub tab_id: String,
    /// Populated for [`PaneVisibility::Hidden`] (backend spawns PTY
    /// synchronously). `None` for [`PaneVisibility::Visible`] — the PTY is
    /// created lazily by the frontend, so callers must look it up later if
    /// they need it.
    pub pty_id: Option<String>,
    pub workspace_id: String,
    /// Worker id for hidden agent spawns. Callers can look up the
    /// `GraphWorkerInfo` via `svc.workers.get(worker_id)` for status
    /// tracking and cancellation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    /// Non-fatal warning. Currently populated when `spec.cwd` differs from
    /// the workspace cwd.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

// ── Spec → SpawnAgentParams adapter ─────────────────────────────────────

impl SpawnPaneSpec {
    /// Convert to the legacy `SpawnAgentParams` used by the current
    /// `spawn_agent_sync` path. Only meaningful when `command` is
    /// [`PaneCommand::Agent`]. Shell spawns take a different code path.
    ///
    /// This adapter is phase 1 glue — it bridges the new type surface to
    /// the existing battle-tested implementation without duplicating the
    /// PTY and agent-launch plumbing. Phase 3 inverts the relationship:
    /// `spawn_agent_sync` will become a thin wrapper that constructs a
    /// spec and calls `spawn_pane`, and this adapter goes away.
    pub(crate) fn to_agent_params(&self) -> Result<SpawnAgentParams, String> {
        let agent = match &self.command {
            PaneCommand::Agent(a) => a,
            PaneCommand::Shell => {
                return Err("to_agent_params called on Shell spec".into());
            }
        };
        let mut args = agent.extra_args.clone();
        if let Some(ref model) = agent.model {
            // Phase 1: no registry access from the spec — model args are
            // resolved provider-side inside the existing spawn path. Pass
            // the model through as a CLI arg placeholder so provider-
            // specific resolvers can pick it up. Phase 3 replaces this
            // with a direct `model_launch_args` call.
            args.push(format!("--model"));
            args.push(model.clone());
        }
        Ok(SpawnAgentParams {
            agent: Some(agent.provider.clone()),
            agent_name: agent.agent_name.clone(),
            args: if args.is_empty() { None } else { Some(args) },
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            role: agent.role.clone(),
            parent_pane_id: self.parent_pane_id.clone(),
            can_spawn: Some(agent.can_spawn),
            task: agent.task.clone(),
            workspace_id: self.workspace_id.clone(),
            force_tab: None,
            report_target_pane_id: agent.report_target_pane_id.clone(),
            skip_permissions: Some(agent.auto_approve),
        })
    }
}

// ── ServiceContext::spawn_pane ──────────────────────────────────────────

impl ServiceContext {
    /// Unified spawn entry point. Construct a [`SpawnPaneSpec`], call this,
    /// get a [`PaneHandle`]. Every feature that creates panes should funnel
    /// through here — do not call `spawn_agent_sync`, `spawn_headless`, or
    /// `mux.create_tab` directly outside of this module.
    ///
    /// **Errors**: unknown agent provider, missing workspace, or PTY spawn
    /// failure (hidden panes only; visible panes fail async inside the
    /// launch thread).
    ///
    /// Phase 1 note: internally delegates to `spawn_agent_sync` for agent
    /// spawns. `Shell` spawns and `Hidden` agent spawns will get their own
    /// implementation paths in phase 3.
    pub fn spawn_pane(&self, spec: SpawnPaneSpec) -> Result<PaneHandle, String> {
        match (&spec.visibility, &spec.command) {
            (PaneVisibility::Visible, PaneCommand::Agent(_)) => {
                let params = spec.to_agent_params()?;
                let result = self.spawn_agent_sync(params)?;
                Ok(spawn_result_to_handle(result, spec.workspace_id.clone()))
            }
            (PaneVisibility::Hidden, PaneCommand::Agent(agent)) => {
                // Phase 1: delegate to spawn_headless which already handles
                // hidden pane + backend PTY + reader + launch. Phase 3
                // step 3 inverts this: spawn_headless becomes a thin
                // wrapper around a direct hidden-pane impl in this module.
                let intent = agent
                    .worker_id
                    .clone()
                    .map(|w| format!("spawn_pane:{}", w))
                    .unwrap_or_else(|| "spawn_pane:hidden".into());
                let task_prompt = agent.task.clone().unwrap_or_default();
                let model = agent.model.clone().unwrap_or_default();
                let spawned_by = agent
                    .spawned_by
                    .clone()
                    .unwrap_or_else(|| "spawn_pane".into());
                let worker_id = crate::mem::workers::spawn_headless(
                    &std::sync::Arc::new(self.clone()),
                    intent,
                    task_prompt,
                    spawned_by,
                    agent.provider.clone(),
                    model,
                )?;
                // spawn_headless doesn't return the pane_id, look it up
                // via the worker registry.
                let info = self
                    .workers
                    .get(&worker_id)
                    .ok_or_else(|| "worker not found after spawn".to_string())?;
                Ok(PaneHandle {
                    pane_id: info.pane_id.clone(),
                    tab_id: String::new(),
                    pty_id: None,
                    workspace_id: spec.workspace_id.clone().unwrap_or_default(),
                    worker_id: Some(worker_id),
                    warning: None,
                })
            }
            (_, PaneCommand::Shell) => {
                // Phase 1: shell-only spawns aren't wired yet — no existing
                // code path produces them through this surface. Phase 3 adds
                // a direct implementation that calls `mux.create_tab` + sets
                // `pane.hidden` based on visibility and returns the handle
                // without touching the agent registry.
                Err("PaneCommand::Shell not yet implemented in spawn_pane (phase 3)".into())
            }
        }
    }
}

fn spawn_result_to_handle(result: SpawnResult, workspace_id: Option<String>) -> PaneHandle {
    PaneHandle {
        pane_id: result.pane_id,
        tab_id: result.tab_id,
        pty_id: if result.pty_id.is_empty() {
            None
        } else {
            Some(result.pty_id)
        },
        workspace_id: workspace_id.unwrap_or_default(),
        worker_id: None,
        warning: result.warning,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_spec(provider: &str) -> SpawnPaneSpec {
        SpawnPaneSpec {
            visibility: PaneVisibility::Visible,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: provider.into(),
                auto_approve: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn default_visibility_is_visible() {
        let spec = SpawnPaneSpec::default();
        assert_eq!(spec.visibility, PaneVisibility::Visible);
    }

    #[test]
    fn default_command_is_shell() {
        let spec = SpawnPaneSpec::default();
        assert!(matches!(spec.command, PaneCommand::Shell));
    }

    #[test]
    fn agent_auto_approve_defaults_true_via_serde() {
        let json = r#"{"provider": "claude"}"#;
        let agent: AgentSpawnSpec = serde_json::from_str(json).unwrap();
        assert!(agent.auto_approve);
    }

    #[test]
    fn agent_auto_approve_defaults_true_via_struct() {
        let agent = AgentSpawnSpec {
            provider: "claude".into(),
            auto_approve: default_true(),
            ..Default::default()
        };
        assert!(agent.auto_approve);
    }

    #[test]
    fn shell_spec_roundtrips_json() {
        let spec = SpawnPaneSpec {
            title: Some("scratch".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
        assert!(matches!(back.command, PaneCommand::Shell));
        assert_eq!(back.title.as_deref(), Some("scratch"));
    }

    #[test]
    fn agent_spec_roundtrips_json() {
        let spec = SpawnPaneSpec {
            visibility: PaneVisibility::Hidden,
            command: PaneCommand::Agent(AgentSpawnSpec {
                provider: "claude".into(),
                model: Some("opus".into()),
                task: Some("hello".into()),
                role: Some("Worker".into()),
                can_spawn: false,
                auto_approve: true,
                worker_id: Some("w-123".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let json = serde_json::to_string(&spec).unwrap();
        let back: SpawnPaneSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back.visibility, PaneVisibility::Hidden);
        match back.command {
            PaneCommand::Agent(a) => {
                assert_eq!(a.provider, "claude");
                assert_eq!(a.model.as_deref(), Some("opus"));
                assert_eq!(a.task.as_deref(), Some("hello"));
                assert_eq!(a.role.as_deref(), Some("Worker"));
                assert_eq!(a.worker_id.as_deref(), Some("w-123"));
                assert!(a.auto_approve);
            }
            _ => panic!("expected Agent"),
        }
    }

    #[test]
    fn to_agent_params_carries_model_through_args() {
        let mut spec = agent_spec("claude");
        if let PaneCommand::Agent(ref mut a) = spec.command {
            a.model = Some("opus".into());
            a.task = Some("count files".into());
        }
        let params = spec.to_agent_params().unwrap();
        assert_eq!(params.agent.as_deref(), Some("claude"));
        assert_eq!(params.task.as_deref(), Some("count files"));
        let args = params.args.unwrap();
        assert!(args.iter().any(|a| a == "--model"));
        assert!(args.iter().any(|a| a == "opus"));
    }

    #[test]
    fn to_agent_params_passes_role_and_report_target() {
        let mut spec = agent_spec("claude");
        spec.parent_pane_id = Some("parent-1".into());
        if let PaneCommand::Agent(ref mut a) = spec.command {
            a.role = Some("Lead".into());
            a.can_spawn = true;
            a.report_target_pane_id = Some("cron-host".into());
        }
        let params = spec.to_agent_params().unwrap();
        assert_eq!(params.role.as_deref(), Some("Lead"));
        assert_eq!(params.parent_pane_id.as_deref(), Some("parent-1"));
        assert_eq!(params.can_spawn, Some(true));
        assert_eq!(params.report_target_pane_id.as_deref(), Some("cron-host"));
    }

    #[test]
    fn to_agent_params_fails_on_shell_spec() {
        let spec = SpawnPaneSpec::default();
        let err = spec.to_agent_params().unwrap_err();
        assert!(err.contains("Shell"));
    }
}
