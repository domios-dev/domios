//! Agent hierarchy: roles, reports, status queries.

use serde::Serialize;
use tauri::Emitter;

use super::{ReportResult, ServiceContext};
use crate::multiplexer::types::AgentRole;

impl ServiceContext {
    pub fn set_pane_role(
        &self,
        pane_id: &str,
        role_str: &str,
        parent_pane_id: Option<&str>,
        can_spawn: Option<bool>,
    ) -> Result<(), String> {
        let role = Self::parse_role(role_str)?;
        let mut mux = self.mux.lock();
        mux.set_pane_role(pane_id, role, parent_pane_id)?;
        if let Some(cs) = can_spawn {
            if let Err(e) = mux.set_can_spawn(pane_id, cs) {
                log::warn!("[hierarchy] set_can_spawn({}) failed: {}", pane_id, e);
            }
        }
        drop(mux);
        self.emit_state_changed();
        Ok(())
    }

    pub fn clear_pane_role(&self, pane_id: &str) -> Result<(), String> {
        self.mux.lock().clear_pane_role(pane_id)?;
        self.emit_state_changed();
        Ok(())
    }

    pub fn set_can_spawn(&self, pane_id: &str, can_spawn: bool) -> Result<(), String> {
        self.mux.lock().set_can_spawn(pane_id, can_spawn)?;
        self.emit_state_changed();
        Ok(())
    }

    /// Send report to the resolved target. Routing priority:
    /// `parent_pane_id` → `report_target_pane_id` → `spawned_by_pane_id`
    /// → global system inbox. NEVER routes back to the reporter's own pane.
    pub fn submit_report(&self, pane_id: &str, message: &str) -> Result<ReportResult, String> {
        let (from_title, target_opt) = {
            let mux = self.mux.lock();
            resolve_report_target(&mux, pane_id)?
        };

        let storage_key = target_opt
            .clone()
            .unwrap_or_else(|| SYSTEM_INBOX_PANE_ID.to_string());
        let report = self
            .report_store
            .lock()
            .add(&storage_key, pane_id, &from_title, message);

        self.events.broadcast(
            "agent.report",
            serde_json::json!({
                "target_pane_id": storage_key,
                "report": report,
            }),
        );
        let _ = self.app_handle.emit(
            "agent-report",
            serde_json::json!({
                "target_pane_id": storage_key,
                "from_pane_id": pane_id,
                "from_title": from_title,
                "message": message,
            }),
        );

        // Inject into the target PTY only when the target is a real pane (parent / report_target / spawner). The system inbox has no PTY, and bouncing
        // back to the reporter's own pane is the bug we just fixed — guard against that explicitly.
        if let Some(ref target_id) = target_opt {
            if target_id != pane_id && self.target_has_live_agent(target_id) {
                let model = self
                    .mux
                    .lock()
                    .get_pane(pane_id)
                    .and_then(|p| p.agent_session_model.clone());
                let pfx = crate::service::msg_prefix::CtxPrefix::report(&from_title, pane_id)
                    .model(model);
                let _ = self.send_text(target_id, &pfx.wrap(message), true);
            }
        }

        Ok(ReportResult {
            report_id: report.id,
            orchestrator_pane_id: storage_key,
        })
    }

    /// True when the target pane currently shows a live agent TUI. A report is
    /// bracketed-paste injected into the target's PTY; if the agent has died and
    /// left a bare shell (stale `agent_type` metadata), pasting the marker makes
    /// the shell execute it as a command. The report is still stored + broadcast
    /// for the chat UI, so gating the PTY inject loses nothing.
    fn target_has_live_agent(&self, target_id: &str) -> bool {
        let pty_id = {
            let mux = self.mux.lock();
            match mux.get_pane(target_id) {
                Some(p) if p.agent_type.is_some() => p.pty_id.clone(),
                _ => return false,
            }
        };
        let Some(pty_id) = pty_id else { return false };
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(&pty_id)
            .unwrap_or_default();
        crate::pty::screen_state::screen_has_agent_tui(&screen)
            || self.agent_registry.any_provider_screen_has_tui(&screen)
    }

    pub fn get_reports(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(pane_id, false);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_and_clear_reports(&self, pane_id: &str) -> Vec<serde_json::Value> {
        let reports = self.report_store.lock().get(pane_id, true);
        serde_json::to_value(&reports)
            .unwrap_or_default()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_pane_status(&self, pane_id: &str) -> Result<PaneStatusInfo, String> {
        let mux = self.mux.lock();
        let pane = mux
            .get_pane(pane_id)
            .ok_or(format!("Pane not found: {}", pane_id))?;

        let status = if let Some(ref pty_id) = pane.pty_id {
            let tracker = self.agent_tracker.lock();
            format!("{:?}", tracker.get_status(pty_id))
        } else {
            "Idle".to_string()
        };

        let children = mux.get_children_ids(pane_id);

        Ok(PaneStatusInfo {
            pane_id: pane.id.clone(),
            status,
            agent_type: pane.agent_type.clone(),
            role: pane.role,
            parent_pane_id: pane.parent_pane_id.clone(),
            can_spawn: pane.can_spawn,
            depth: pane.depth,
            children,
            cwd: pane.cwd.clone(),
            title: pane.title.clone(),
        })
    }
}

/// Storage key used when a report cannot be routed to any specific pane
/// (no parent, no report_target, no spawner). Reachable via
/// `agent reports --pane __system__` and via the agent-report event stream.
pub const SYSTEM_INBOX_PANE_ID: &str = "__system__";

/// Resolve where a report from `pane_id` should be delivered.
///
/// Returns `(from_title, Some(target_pane_id))` when a real target exists,
/// or `(from_title, None)` when the report should land in the global system
/// inbox. The resolution order is:
///
/// 1. `parent_pane_id` — direct parent in the role hierarchy (cascades up)
/// 2. `report_target_pane_id` — cron / mem worker non-hierarchy target
/// 3. `spawned_by_pane_id` — agent that ran `agent spawn`, even when no
///    role parent (top-level Orchestrator spawned by another agent)
///
/// The reporter's own pane id is filtered out at every level: a pane must
/// never resolve to itself, otherwise reports get injected into the
/// reporter's own buffer and never reach the spawning agent.
pub fn resolve_report_target(
    mux: &crate::multiplexer::state::MultiplexerState,
    pane_id: &str,
) -> Result<(String, Option<String>), String> {
    let pane = mux.get_pane(pane_id).ok_or("Pane not found")?;
    let title = pane.title.clone();
    let target = [
        pane.parent_pane_id.as_deref(),
        pane.report_target_pane_id.as_deref(),
        pane.spawned_by_pane_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|candidate| !candidate.is_empty() && *candidate != pane_id)
    .map(str::to_string);
    Ok((title, target))
}

#[derive(Debug, Clone, Serialize)]
pub struct PaneStatusInfo {
    pub pane_id: String,
    pub status: String,
    pub agent_type: Option<String>,
    pub role: Option<AgentRole>,
    pub parent_pane_id: Option<String>,
    pub can_spawn: bool,
    pub depth: u8,
    pub children: Vec<String>,
    pub cwd: Option<String>,
    pub title: String,
}

#[cfg(test)]
mod tests {
    use super::resolve_report_target;
    use crate::agent::reports::ReportStore;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{AgentRole, Orientation};

    // Enforce report key symmetry: reporter writes and parent query must use the same key.
    // This avoids "report succeeded" with empty parent results.

    fn build_orchestrator_with_worker(worker_title: &str) -> (MultiplexerState, String, String) {
        let (mut mux, orch) = MultiplexerState::new();
        let worker = mux.create_pane();
        mux.set_pane_role(&worker, AgentRole::Worker, Some(&orch))
            .expect("attach worker to orchestrator");
        mux.rename_pane(&worker, worker_title)
            .expect("rename worker pane");
        (mux, orch, worker)
    }

    #[test]
    fn worker_report_is_listed_by_orchestrator() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-1");
        let mut store = ReportStore::new(100);

        let (from_title, target_id) = resolve_report_target(&mux, &worker).expect("resolve target");
        let target_id = target_id.expect("worker has a parent target");
        assert_eq!(target_id, orch, "worker reports to its direct parent");
        assert_eq!(from_title, "Worker-1");

        store.add(&target_id, &worker, &from_title, "task done");

        let listed = store.get(&orch, false);
        assert_eq!(
            listed.len(),
            1,
            "orchestrator must see the worker's report after submit"
        );
        assert_eq!(listed[0].from_pane_id, worker);
        assert_eq!(listed[0].from_title, "Worker-1");
        assert_eq!(listed[0].message, "task done");
    }

    #[test]
    fn worker_report_keys_match_storage_and_query() {
        let (mux, orch, worker) = build_orchestrator_with_worker("Worker-A");
        let mut store = ReportStore::new(100);

        let (from_title, storage_key) =
            resolve_report_target(&mux, &worker).expect("resolve target");
        let storage_key = storage_key.expect("worker has a parent target");
        store.add(&storage_key, &worker, &from_title, "msg");

        let query_key = &orch;
        assert_eq!(
            &storage_key, query_key,
            "submit_report write key and agent.reports query key must agree"
        );
        assert_eq!(store.get(query_key, false).len(), 1);
    }

    #[test]
    fn cron_report_target_is_used_when_no_parent() {
        let (mut mux, orch) = MultiplexerState::new();
        let cron_pane = mux.create_pane();
        // Cron-spawned panes carry an explicit report_target_pane_id but no role.
        if let Some(pane) = mux.get_pane_mut(&cron_pane) {
            pane.report_target_pane_id = Some(orch.clone());
        }
        mux.rename_pane(&cron_pane, "cron-task").unwrap();

        let (from_title, target) = resolve_report_target(&mux, &cron_pane).unwrap();
        assert_eq!(target.as_deref(), Some(orch.as_str()));
        assert_eq!(from_title, "cron-task");
    }

    #[test]
    fn parent_pane_id_takes_priority_over_other_targets() {
        let (mut mux, orch, worker) = build_orchestrator_with_worker("Worker-Pri");
        let other = mux.create_pane();
        let spawner = mux.create_pane();
        if let Some(pane) = mux.get_pane_mut(&worker) {
            pane.report_target_pane_id = Some(other.clone());
            pane.spawned_by_pane_id = Some(spawner.clone());
        }

        let (_, target) = resolve_report_target(&mux, &worker).unwrap();
        assert_eq!(
            target.as_deref(),
            Some(orch.as_str()),
            "explicit parent_pane_id wins over report_target and spawned_by"
        );
    }

    #[test]
    fn orchestrator_falls_back_to_spawned_by_when_no_parent() {
        let (mut mux, orch) = MultiplexerState::new();
        // Simulate the spawning agent — it lives in its own pane elsewhere.
        let spawner = mux.create_pane();
        // Top-level orchestrator has no parent and no report_target, but `agent spawn` recorded who asked for it.
        if let Some(pane) = mux.get_pane_mut(&orch) {
            pane.spawned_by_pane_id = Some(spawner.clone());
        }

        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert_eq!(
            target.as_deref(),
            Some(spawner.as_str()),
            "top-level orchestrator routes its report to whoever spawned it"
        );
    }

    #[test]
    fn never_routes_back_to_self_when_all_targets_unset() {
        // Top-level orchestrator with no parent / no report_target / no spawner. The previous implementation walked the parent chain and returned the
        // pane's own id, so reports got injected into the orchestrator's own PTY and `agent reports` against the spawning agent stayed empty.
        let (mux, orch) = MultiplexerState::new();
        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert!(
            target.is_none(),
            "orphan orchestrator must NOT resolve to itself; got {:?}",
            target
        );
    }

    #[test]
    fn never_routes_back_to_self_even_when_targets_point_at_self() {
        // Defensive: if any field accidentally points at the reporter itself, that field is skipped — reports should never bounce.
        let (mut mux, orch) = MultiplexerState::new();
        if let Some(pane) = mux.get_pane_mut(&orch) {
            pane.parent_pane_id = Some(orch.clone());
            pane.report_target_pane_id = Some(orch.clone());
            pane.spawned_by_pane_id = Some(orch.clone());
        }
        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert!(
            target.is_none(),
            "self-referential targets must be filtered out; got {:?}",
            target
        );
    }

    #[test]
    fn spawned_by_used_only_when_parent_and_report_target_missing() {
        let (mut mux, orch) = MultiplexerState::new();
        let spawner = mux.create_pane();
        let report_target = mux.create_pane();
        if let Some(pane) = mux.get_pane_mut(&orch) {
            pane.report_target_pane_id = Some(report_target.clone());
            pane.spawned_by_pane_id = Some(spawner.clone());
        }
        let (_, target) = resolve_report_target(&mux, &orch).unwrap();
        assert_eq!(
            target.as_deref(),
            Some(report_target.as_str()),
            "report_target_pane_id wins over spawned_by_pane_id"
        );
    }

    #[test]
    fn unknown_pane_id_is_rejected() {
        let (mux, _orch) = MultiplexerState::new();
        let err = resolve_report_target(&mux, "nonexistent").unwrap_err();
        assert!(err.contains("Pane not found"), "got: {}", err);
    }

    // ── Report message format ── submit_report builds the notify body without a trailing CR — Enter is sent as a separate delayed raw write by send_text's bracket-paste path.

    fn format_report(from_title: &str, message: &str) -> String {
        format!("[Report from {}] {}", from_title, message)
    }

    #[test]
    fn report_format_basic() {
        let msg = format_report("Scout", "Task completed successfully");
        assert_eq!(msg, "[Report from Scout] Task completed successfully");
    }

    #[test]
    fn report_format_no_trailing_cr() {
        // Enter is delivered as a separate write by send_text — the body itself must NOT contain trailing \r, otherwise Claude's prompt submits before
        // the bracket-paste end marker is processed.
        let msg = format_report("Worker-1", "done");
        assert!(!msg.ends_with('\r'), "Report body must not end with \\r");
    }

    #[test]
    fn report_format_message_with_newlines() {
        // Internal newlines pass through raw — bracket paste protects them.
        let msg = format_report("Worker", "line1\nline2\nline3");
        assert_eq!(msg, "[Report from Worker] line1\nline2\nline3");
        assert!(msg.contains('\n'), "Internal newlines are preserved");
    }

    #[test]
    fn report_format_empty_message() {
        let msg = format_report("Agent", "");
        assert_eq!(msg, "[Report from Agent] ");
    }

    #[test]
    fn report_format_title_with_spaces() {
        let msg = format_report("My Agent Title", "ok");
        assert_eq!(msg, "[Report from My Agent Title] ok");
    }

    #[test]
    fn report_format_message_with_special_chars() {
        let msg = format_report("Bot", "found 3 files: a.rs, b.rs, c.rs");
        assert!(msg.starts_with("[Report from Bot] "));
    }
}
