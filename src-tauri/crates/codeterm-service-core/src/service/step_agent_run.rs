//! Blueprint step agent runs: one prompt answered by the calling provider plugin's agent in a
//! hidden tab, the way cron runs spawn theirs.
//!
//! Host API audit (`host.agentRun`):
//! - The provider is the calling plugin's own agentProvider registration (provider id == plugin
//!   id); a plugin never names a provider and core holds no provider default.
//! - Workspace, Team and owner come from the host-stamped step request, never from the plugin.
//! - Cwd must equal the authored step param before trust is answered; ended tabs always close.
//! - The prompt is the hidden launch's first turn, delivered once by the launch path; nothing
//!   here resends it.
//! - Completion is the run watcher's turn-end event. The reply is the last assistant message of
//!   the tab's own claimed session, bounded to `MAX_REPLY_BYTES`.
//! - Runs live in this process only: after a restart a run is `unknown` and never resubmitted.

use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::{TabAudience, TabCloseReason};
use crate::service::spawn::{RuntimeOwnerLinkage, SpawnTabSpec, TabVisibility};
use crate::service::{ServiceContext, SpawnAgentParams};
use parking_lot::{Condvar, Mutex};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Instant;

pub const MAX_REPLY_BYTES: usize = 60 * 1024;
const MAX_PROMPT_BYTES: usize = 48 * 1024;
fn step_agent_title(_label: &str) -> String {
    "Prompt".into()
}

const TRUNCATED: &str = "\n[truncated]";
// Ended runs stay readable for later reconcile calls, then are forgotten (reported as unknown).
const ENDED_RETENTION: std::time::Duration = std::time::Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepRunState {
    Running,
    Done { text: String },
    Failed { reason: String },
    Canceled,
    Unknown,
}

impl StepRunState {
    fn terminal(&self) -> bool {
        !matches!(self, Self::Running | Self::Unknown)
    }
}

struct StepRun {
    plugin: String,
    operation: String,
    tab_id: Option<String>,
    state: StepRunState,
    ended_at: Option<Instant>,
}

#[derive(Default)]
struct StepRuns {
    runs: Mutex<HashMap<String, StepRun>>,
    changed: Condvar,
}

static RUNS: LazyLock<StepRuns> = LazyLock::new(StepRuns::default);

/// What a step asks for; scope fields are copied from the host-stamped step request.
pub struct StepRunRequest {
    pub plugin: String,
    pub operation: String,
    pub workspace: String,
    pub team: Option<String>,
    pub owner: String,
    pub label: String,
    pub prompt: String,
    pub model: Option<String>,
    pub cwd: String,
    pub timeout_ms: u64,
    pub sandbox: Option<String>,
}

/// The canonical existing absolute directory, or why it is refused.
pub fn working_dir(raw: &str) -> Result<String, String> {
    let path = std::path::Path::new(raw);
    if !path.is_absolute() {
        return Err(format!("cwd must be an absolute directory: {raw}"));
    }
    let canonical = std::fs::canonicalize(path)
        .ok()
        .filter(|dir| dir.is_dir())
        .ok_or_else(|| format!("cwd is not an existing directory: {raw}"))?;
    let shown = canonical.to_string_lossy();
    // Windows canonical paths carry the verbatim prefix, which shells and providers reject.
    Ok(shown
        .strip_prefix(r"\\?\")
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .unwrap_or(&shown)
        .to_string())
}

/// The last assistant message after the final user turn, bounded with an explicit marker.
pub fn final_reply(messages: &[ChatMsg]) -> Option<String> {
    let last = messages.iter().rev().find(|message| {
        matches!(
            message.kind.as_str(),
            "assistant" | "user" | "tool" | "tool_call" | "tool_use" | "tool_result"
        )
    })?;
    if last.kind != "assistant" || last.content.trim().is_empty() {
        return None;
    }
    Some(bounded(last.content.trim()))
}

fn bounded(text: &str) -> String {
    if text.len() <= MAX_REPLY_BYTES {
        return text.to_string();
    }
    let mut end = MAX_REPLY_BYTES - TRUNCATED.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{TRUNCATED}", &text[..end])
}

fn settle(run_id: &str, state: StepRunState) -> Option<Option<String>> {
    let mut runs = RUNS.runs.lock();
    let run = runs.get_mut(run_id)?;
    if run.state.terminal() {
        return None;
    }
    run.state = state;
    run.ended_at = Some(Instant::now());
    let ended = run.tab_id.clone();
    drop(runs);
    RUNS.changed.notify_all();
    Some(ended)
}

fn close_run(svc: &ServiceContext, ended: Option<Option<String>>) {
    if let Some(Some(tab)) = ended {
        if let Err(error) = svc.close_tab_with_reason(&tab, TabCloseReason::AgentRunEnded) {
            log::debug!("[step-run] close {tab}: {error}");
        }
    }
}

/// The run watcher's end of a step run: the reply comes from the tab's own session messages.
pub(crate) fn finish(
    svc: &ServiceContext,
    run_id: &str,
    success: bool,
    summary: String,
    messages: Result<Vec<ChatMsg>, String>,
) {
    let state = if !success {
        StepRunState::Failed { reason: summary }
    } else {
        match messages.map(|messages| final_reply(&messages)) {
            Ok(Some(text)) => StepRunState::Done { text },
            Ok(None) => StepRunState::Failed {
                reason: "the agent recorded no final reply".into(),
            },
            Err(error) => StepRunState::Failed {
                reason: format!("the agent reply could not be read: {error}"),
            },
        }
    };
    close_run(svc, settle(run_id, state));
}

/// Own-session messages of a PTY-backed step tab, read before the tab closes.
pub(crate) fn pty_messages(svc: &ServiceContext, tab_id: &str) -> Result<Vec<ChatMsg>, String> {
    let (provider, cwd, session) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("step tab closed")?;
        (
            tab.agent_type.clone().ok_or("step provider missing")?,
            tab.cwd.clone().unwrap_or_default(),
            tab.agent_session_id.clone().ok_or("step session missing")?,
        )
    };
    crate::service::tab_chat::session_chat_snapshot(svc, &provider, &cwd, &session, 0)
        .map(|(messages, _)| messages)
}

fn step_permission_bypass(
    svc: &ServiceContext,
    plugin: &str,
    confined: bool,
) -> Result<Option<super::permission_policy::PermissionBypassAuthorization>, String> {
    super::permission_policy::spawn_permission_bypass_for(
        svc,
        &svc.agent_registry,
        plugin,
        false,
        confined,
    )
}

impl ServiceContext {
    /// Spawns the hidden step agent and returns its run id; the prompt is its first turn.
    pub fn start_step_agent_run(&self, request: StepRunRequest) -> Result<String, String> {
        if self
            .agent_registry
            .with_provider(&request.plugin, |_| ())
            .is_none()
        {
            return Err("this plugin registers no agent provider".into());
        }
        if request.prompt.trim().is_empty() || request.prompt.len() > MAX_PROMPT_BYTES {
            return Err(format!("prompt must be 1..{MAX_PROMPT_BYTES} bytes"));
        }
        let cwd = working_dir(&request.cwd)?;
        let permission_bypass =
            step_permission_bypass(self, &request.plugin, request.sandbox.is_some())?;
        let run_id = uuid::Uuid::new_v4().simple().to_string();
        let mut params = SpawnAgentParams {
            agent: Some(request.plugin.clone()),
            model: request.model.filter(|model| !model.trim().is_empty()),
            task: Some(request.prompt),
            // The Blueprint author chose this directory, as a cron author does, so its trust prompt is answered.
            provisioned_cwd: Some(cwd.clone()),
            cwd: Some(cwd),
            workspace_id: Some(request.workspace),
            team_id: request.team,
            creator_tab_id: Some(request.owner.clone()),
            title: Some(step_agent_title(&request.label)),
            audience: TabAudience::UserOwnedAutomation,
            skip_permissions: Some(permission_bypass.is_some()),
            permission_bypass_authorization: permission_bypass,
            sandbox: request.sandbox,
            spawned_by_tab_id: Some(request.owner),
            ..Default::default()
        };
        crate::service::move_chatbackend_task_to_system_prompt(
            self.chatbackend_registry.as_ref(),
            &request.plugin,
            &mut params,
        );
        let mut spec = SpawnTabSpec::from_agent_params_with_chatbackends(
            self.chatbackend_registry.as_ref(),
            params,
        );
        spec.visibility = TabVisibility::Hidden;
        if let Some(metadata) = spec.chatbackend_launch.as_mut() {
            metadata.owner_linkage = Some(RuntimeOwnerLinkage::StepRun {
                run_id: run_id.clone(),
                timeout_seconds: request.timeout_ms.div_ceil(1000),
            });
        }
        if let Some(agent) = spec.agent_launch_mut() {
            agent.spawned_by = Some(format!("blueprint-step:{}", request.plugin));
        }
        {
            let mut runs = RUNS.runs.lock();
            runs.retain(|_, run| run.ended_at.is_none_or(|at| at.elapsed() < ENDED_RETENTION));
            if let Some((id, _)) = runs
                .iter()
                .find(|(_, run)| run.plugin == request.plugin && run.operation == request.operation)
            {
                return Ok(id.clone());
            }
            if runs
                .values()
                .filter(|run| run.plugin == request.plugin && !run.state.terminal())
                .count()
                >= 32
            {
                return Err("too many active step agent runs".into());
            }
            runs.insert(
                run_id.clone(),
                StepRun {
                    plugin: request.plugin.clone(),
                    operation: request.operation.clone(),
                    tab_id: None,
                    state: StepRunState::Running,
                    ended_at: None,
                },
            );
        }
        let runtime = crate::service::agent_runtime::engine::requested(self, &request.plugin);
        let handle = match self.spawn_tab(spec) {
            Ok(handle) => handle,
            Err(error) => {
                RUNS.runs.lock().remove(&run_id);
                return Err(error);
            }
        };
        let ended_early = if let Some(run) = RUNS.runs.lock().get_mut(&run_id) {
            run.tab_id = Some(handle.tab_id.clone());
            run.state.terminal()
        } else {
            false
        };
        if ended_early {
            let _ = self.close_tab_with_reason(&handle.tab_id, TabCloseReason::AgentRunEnded);
            return Ok(run_id);
        }
        if !runtime {
            let pty = self.agent_run_pty_map.lock().get(&handle.tab_id).cloned();
            let Some(pty) = pty else {
                close_run(
                    self,
                    settle(
                        &run_id,
                        StepRunState::Failed {
                            reason: "the step agent has no terminal".into(),
                        },
                    ),
                );
                return Ok(run_id);
            };
            self.run_watcher.register(
                self.clone(),
                pty,
                handle.tab_id,
                crate::agent::run_watcher::AgentRunKind::StepRun {
                    run_id: run_id.clone(),
                },
                request.timeout_ms.div_ceil(1000),
                None,
            );
        }
        Ok(run_id)
    }

    /// Waits for the run's end event until `deadline`; `running` when the deadline comes first.
    pub fn wait_step_agent_run(
        &self,
        plugin: &str,
        operation: &str,
        run_id: &str,
        deadline: Instant,
    ) -> StepRunState {
        let mut runs = RUNS.runs.lock();
        loop {
            let Some(run) = runs
                .get(run_id)
                .filter(|run| run.plugin == plugin && run.operation == operation)
            else {
                return StepRunState::Unknown;
            };
            if run.state.terminal() {
                return run.state.clone();
            }
            if RUNS.changed.wait_until(&mut runs, deadline).timed_out() {
                return runs
                    .get(run_id)
                    .map_or(StepRunState::Unknown, |run| run.state.clone());
            }
        }
    }

    /// Interrupts a runtime turn when the provider has one, then closes the tab.
    pub fn cancel_step_agent_run(
        &self,
        plugin: &str,
        operation: &str,
        run_id: &str,
    ) -> StepRunState {
        let tab = match RUNS
            .runs
            .lock()
            .get(run_id)
            .filter(|run| run.plugin == plugin && run.operation == operation)
        {
            None => return StepRunState::Unknown,
            Some(run) if run.state.terminal() => return run.state.clone(),
            Some(run) => run.tab_id.clone(),
        };
        if let Some(tab) = &tab {
            if crate::service::agent_runtime::engine::for_tab(tab).is_some() {
                let _ = crate::service::agent_runtime::controls::interrupt(self, tab);
            }
        }
        let ended = settle(run_id, StepRunState::Canceled);
        if let Some(Some(tab)) = ended {
            let _ = self.close_tab_with_reason(&tab, TabCloseReason::Cancelled);
        }
        StepRunState::Canceled
    }
}

#[cfg(test)]
mod tests;
