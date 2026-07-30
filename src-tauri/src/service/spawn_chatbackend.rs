//! Spawn path for non-PTY `chatBackend` tabs (`pty_id: None`, no backend
//! reader); inbound transcript arrives via the plugin `poll` → `StructuredSink`.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use super::spawn::{SpawnTabSpec, TabCommand, TabHandle};
use super::ServiceContext;
use crate::agent::chat_types::{ChatMsg, StructuredChat};
use crate::agent::reports::ReportDetails;
use crate::multiplexer::types::{TabBacking, WatcherPolicy, WatcherSpec};
use crate::service::messenger::MessageEnvelope;
use crate::service::watcher::{
    apply_guardrails, build_observation, collect_watch_group, observation_config_from_json,
    parse_verdict, ObservationSources, VerdictOutcome, WatcherAction, WatcherVerdictStatus,
};

/// Adaptive inbound-poll cadence bounds: fast while streaming, geometric
/// back-off to idle so an open-but-quiet chat tab costs almost nothing.
const POLL_ACTIVE_MS: u64 = 60;
const POLL_IDLE_MS: u64 = 750;

fn next_poll_delay_ms(had_delta: bool, current_ms: u64) -> u64 {
    if had_delta {
        POLL_ACTIVE_MS
    } else {
        current_ms
            .saturating_mul(2)
            .clamp(POLL_ACTIVE_MS, POLL_IDLE_MS)
    }
}

/// Byte-identity signature of a delta's payload, so a streaming message
/// re-returned unchanged hashes the same and the redundant emit is suppressed.
fn delta_signature(delta: &StructuredChat) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&delta.messages)
        .unwrap_or_default()
        .hash(&mut h);
    h.finish()
}

struct PollTick {
    cursor: Option<String>,
    emit: bool,
    delay_ms: u64,
    signature: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatcherTickPlan {
    Arm { tick: u64 },
    BlockedInflight,
    Wait,
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn watcher_interval_ms(policy: &WatcherPolicy) -> u64 {
    policy.tick_interval_secs.saturating_mul(1000)
}

fn watcher_watchdog_timeout_ms(policy: &WatcherPolicy) -> u64 {
    watcher_interval_ms(policy).saturating_mul(2).max(300_000)
}

fn plan_watcher_tick(spec: &WatcherSpec, now_ms: u64, plugin_busy: bool) -> WatcherTickPlan {
    if spec.tick_inflight_since_ms.is_some() {
        return WatcherTickPlan::BlockedInflight;
    }
    if plugin_busy {
        return WatcherTickPlan::Wait;
    }
    let interval_ms = watcher_interval_ms(&spec.policy);
    let due = spec
        .last_tick_ms
        .map(|last| now_ms >= last.saturating_add(interval_ms))
        .unwrap_or(true);
    if due {
        WatcherTickPlan::Arm {
            tick: spec.tick.saturating_add(1),
        }
    } else {
        WatcherTickPlan::Wait
    }
}

fn watcher_inflight_stale(spec: &WatcherSpec, now_ms: u64) -> bool {
    spec.tick_inflight_since_ms.is_some_and(|since| {
        now_ms.saturating_sub(since) >= watcher_watchdog_timeout_ms(&spec.policy)
    })
}

fn clear_stale_watcher_inflight_spec(spec: &mut WatcherSpec, now_ms: u64) -> bool {
    if watcher_inflight_stale(spec, now_ms) {
        spec.tick_inflight_since_ms = None;
        spec.last_tick_ms = Some(now_ms);
        return true;
    }
    false
}

fn accept_watcher_verdict_id(
    last_ids: &mut HashMap<String, String>,
    tab_id: &str,
    verdict_id: &str,
) -> bool {
    if last_ids.get(tab_id).is_some_and(|last| last == verdict_id) {
        return false;
    }
    last_ids.insert(tab_id.to_string(), verdict_id.to_string());
    true
}

fn apply_watcher_outcome_to_spec(
    spec: &mut WatcherSpec,
    outcome: VerdictOutcome,
    now_ms: u64,
) -> bool {
    if spec.tick_inflight_since_ms.is_none() {
        return false;
    }
    spec.state = outcome.state;
    spec.last_status = Some(watcher_status_label(outcome.verdict.status).to_string());
    spec.last_tick_ms = Some(now_ms);
    spec.tick_inflight_since_ms = None;
    true
}

fn watcher_system_note(text: impl Into<String>) -> ChatMsg {
    ChatMsg {
        id: format!("watcher-note-{}", uuid::Uuid::new_v4().simple()),
        kind: "system".to_string(),
        content: text.into(),
        system_subtype: Some("watcher".to_string()),
        ..Default::default()
    }
}

fn watcher_status_label(status: WatcherVerdictStatus) -> &'static str {
    match status {
        WatcherVerdictStatus::Ok => "ok",
        WatcherVerdictStatus::Attention => "attention",
        WatcherVerdictStatus::Stalled => "stalled",
    }
}

fn extract_watcher_verdicts(delta: &StructuredChat) -> Vec<(String, String)> {
    delta
        .messages
        .iter()
        .filter(|m| m.kind == "watcher_verdict")
        .map(|m| (m.id.clone(), m.content.clone()))
        .collect()
}

fn tick_input_json(
    tick: u64,
    now_ms: u64,
    state: serde_json::Value,
    observations: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "tick": tick,
        "nowMs": now_ms,
        "state": state,
        "observations": observations,
    })
}

fn execute_watcher_actions_with<N, O, R>(
    tab_id: &str,
    actions: &[WatcherAction],
    now_ms: u64,
    cooldowns: &mut HashMap<String, u64>,
    mut nudge: N,
    mut notify: O,
    mut report: R,
) where
    N: FnMut(&str, &str) -> Result<(), String>,
    O: FnMut(&str),
    R: FnMut(&str) -> Result<(), String>,
{
    for action in actions {
        match action.kind.as_str() {
            "nudge" => {
                if let Some(target) = action.tab.as_deref() {
                    match nudge(target, &action.message) {
                        Ok(()) => {
                            cooldowns.insert(target.to_string(), now_ms);
                        }
                        Err(e) => log::warn!(
                            "[watcher] nudge from {} to {} failed: {}",
                            tab_id,
                            target,
                            e
                        ),
                    }
                }
            }
            "notify" => notify(&action.message),
            "report" => {
                if let Err(e) = report(&action.message) {
                    log::warn!("[watcher] report from {} failed: {}", tab_id, e);
                }
            }
            _ => {}
        }
    }
}

/// Nudge targets include the orchestrator itself — the canonical health
/// watcher nudges its own orchestrator, which is not a descendant.
fn guardrail_group(watch: crate::service::watcher::WatchGroup) -> HashSet<String> {
    let mut group: HashSet<String> = watch.tab_ids.into_iter().collect();
    group.insert(watch.orchestrator_id);
    group
}

/// Pure per-tick planning for the poll loop. Emit only when the delta is
/// non-empty AND differs from the last emitted payload — a streaming message
/// re-returned unchanged is suppressed to avoid redundant IPC + O(n²) bytes.
fn plan_poll_tick(
    delta: &StructuredChat,
    prev_cursor: Option<String>,
    prev_delay: u64,
    prev_signature: Option<u64>,
) -> PollTick {
    let had_delta = !delta.messages.is_empty();
    let signature = if had_delta {
        Some(delta_signature(delta))
    } else {
        prev_signature
    };
    PollTick {
        cursor: delta.cursor.clone().or(prev_cursor),
        emit: had_delta && signature != prev_signature,
        delay_ms: next_poll_delay_ms(had_delta, prev_delay),
        signature,
    }
}

/// Spawn on the daemon's global tauri runtime. The synchronous socket-RPC spawn
/// path has no ambient Tokio runtime, where bare `tokio::spawn` would panic.
fn spawn_bg<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    tauri::async_runtime::spawn(fut);
}

impl ServiceContext {
    pub(crate) fn spawn_chatbackend_impl_with_state_change(
        &self,
        spec: SpawnTabSpec,
        emit_state_change: bool,
    ) -> Result<TabHandle, String> {
        let (plugin_id, config, role, role_profile, can_spawn, watcher) = match &spec.command {
            TabCommand::ChatBackend {
                plugin_id,
                config,
                role,
                role_profile,
                can_spawn,
                watcher,
            } => (
                plugin_id.clone(),
                config.clone(),
                role.clone(),
                role_profile.clone(),
                *can_spawn,
                watcher.clone(),
            ),
            _ => return Err("spawn_chatbackend_impl called without ChatBackend command".into()),
        };

        let (ws_id, ws_cwd) = {
            let mux = self.mux.write();
            let id = crate::service::spawn::resolve_spawn_workspace_id(
                spec.workspace_id.clone(),
                spec.parent_tab_id.as_deref(),
                |pid| mux.find_workspace_for_tab(pid),
                mux.get_active_workspace().map(|w| w.id.clone()),
                mux.list_workspaces().first().map(|w| w.id.clone()),
            )?;
            let cwd = mux.get_workspace(&id).and_then(|ws| ws.cwd.clone());
            (id, cwd)
        };
        let resolved_cwd = spec.cwd.clone().or(ws_cwd);

        let tab_id = {
            let mut mux = self.mux.write();
            let result = mux.create_tab(&ws_id)?;
            if let Some(ref title) = spec.title {
                if let Err(e) = mux.rename_tab(&result, title) {
                    log::warn!("[spawn.chatbackend] rename_tab failed: {}", e);
                }
            }
            if let Some(ref description) = spec.description {
                let _ = mux.set_tab_description(&result, Some(description.clone()));
            }
            result
        };

        // Persisted as the JSONL `_meta` header so a restored tab can re-open from the same config.
        let history_meta = config.clone();

        // Tab shows instantly with an empty session id; openSession runs in the background.
        {
            let mut mux = self.mux.write();
            if let Some(tab) = mux.get_tab_mut(&tab_id) {
                tab.backing = TabBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: String::new(),
                };
                tab.chat_view = true;
                tab.role_profile = role_profile.clone();
                tab.watcher = watcher.clone();
                if let Some(ref dir) = resolved_cwd {
                    tab.cwd = Some(dir.clone());
                }
            }
        }
        if let Some(ref role) = role {
            let role_result = if emit_state_change {
                self.set_tab_role(&tab_id, role, spec.parent_tab_id.as_deref(), can_spawn)
            } else {
                let parsed = Self::parse_role(role)?;
                let mut mux = self.mux.write();
                mux.set_tab_role(&tab_id, parsed, spec.parent_tab_id.as_deref())?;
                if let Some(can_spawn) = can_spawn {
                    mux.set_can_spawn(&tab_id, can_spawn)?;
                }
                Ok(())
            };
            if let Err(e) = role_result {
                let _ = self.mux.write().close_tab(&tab_id);
                if emit_state_change {
                    self.emit_state_changed();
                }
                return Err(e);
            }
        }
        if role.is_none() && emit_state_change {
            self.emit_state_changed();
        }

        self.chatbackend_launch_async(plugin_id, tab_id.clone(), config, history_meta);

        let budget_notice = self.workspace_tab_budget_notice(&ws_id);
        Ok(TabHandle {
            tab_id,
            pty_id: None,
            workspace_id: ws_id,
            description: spec.description,
            worker_id: None,
            warning: None,
            model_fallback: None,
            budget_notice,
        })
    }

    /// Background launch: open the plugin session, stamp its id onto the backing,
    /// publish models, start the poll loop. On openSession failure the tab is
    /// torn down so a bad config leaves no dangling empty tab.
    pub(crate) fn chatbackend_launch_async(
        &self,
        plugin_id: String,
        tab_id: String,
        config: serde_json::Value,
        history_meta: serde_json::Value,
    ) {
        let svc = self.clone();
        spawn_bg(async move {
            let (open_svc, open_plugin, open_tab, open_cfg) =
                (svc.clone(), plugin_id.clone(), tab_id.clone(), config);
            let session_id = match tokio::task::spawn_blocking(move || {
                open_svc.chatbackend_open_session(&open_plugin, &open_tab, open_cfg)
            })
            .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    log::warn!(
                        "[spawn.chatbackend] openSession failed for {}: {}",
                        tab_id,
                        e
                    );
                    let _ = svc.mux.write().close_tab(&tab_id);
                    svc.emit_state_changed();
                    svc.create_notification(
                        format!("Failed to open {} session", plugin_id),
                        Some(e),
                        Some("error"),
                    );
                    return;
                }
                Err(e) => {
                    log::warn!("[spawn.chatbackend] openSession task join error: {}", e);
                    let _ = svc.mux.write().close_tab(&tab_id);
                    svc.emit_state_changed();
                    return;
                }
            };

            {
                let mut mux = svc.mux.write();
                if let Some(tab) = mux.get_tab_mut(&tab_id) {
                    if let TabBacking::ChatBackend {
                        session_id: sid, ..
                    } = &mut tab.backing
                    {
                        *sid = session_id.clone();
                    }
                }
            }
            if let Err(error) =
                svc.bind_in_process_chatbackend_session(&tab_id, &plugin_id, &session_id)
            {
                log::warn!(
                    "[spawn.chatbackend] failed to bind provider session for {}: {}",
                    tab_id,
                    error
                );
                let _ = svc.chatbackend_close(&plugin_id, &session_id);
                let _ = svc.mux.write().close_tab(&tab_id);
                svc.emit_state_changed();
                return;
            }
            svc.emit_state_changed();

            log::info!(
                "[spawn.chatbackend] Tab {} backed by plugin '{}' session '{}'",
                tab_id,
                plugin_id,
                session_id
            );

            // Publish models for the tab picker; listModels may hit a network backend, so spawn_blocking.
            {
                let svc2 = svc.clone();
                let plugin2 = plugin_id.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Ok(models) = svc2.chatbackend_list_models(&plugin2) {
                        if !models.is_empty() {
                            crate::agent::model_catalog::register(
                                &plugin2,
                                crate::agent::model_catalog::RegisteredDefaults {
                                    models,
                                    default: None,
                                },
                            );
                        }
                    }
                });
            }

            let replay_tab = tab_id.clone();
            svc.spawn_chatbackend_poll_loop(tab_id, plugin_id, session_id, history_meta);
            svc.schedule_planner_replay(&replay_tab);
        });
    }

    /// Background loop: poll the plugin and emit new transcript on
    /// `tab-chat-msg-{id}`, ending when the tab closes. Poll runs on a blocking
    /// thread so a slow plugin can't stall the async runtime.
    pub(crate) fn spawn_chatbackend_poll_loop(
        &self,
        tab_id: String,
        plugin_id: String,
        session_id: String,
        history_meta: serde_json::Value,
    ) {
        let svc = self.clone();
        let app = self.app_handle.clone();
        spawn_bg(async move {
            let (source_session_id, session_binding_nonce) = svc
                .mux
                .read()
                .get_tab(&tab_id)
                .map(|tab| {
                    (
                        tab.agent_session_id.clone(),
                        tab.session_binding_nonce.clone(),
                    )
                })
                .unwrap_or((None, None));
            let inner = crate::service::tab_chat::structured_sink::make_sink(
                app,
                source_session_id,
                session_binding_nonce,
                Some(svc.chat_send_ledger.clone()),
            );
            // Durable wrapper: forwards to the UI sink and UPSERTs each delta to the session JSONL by id.
            let sink: std::sync::Arc<
                dyn crate::service::tab_chat::structured_sink::StructuredSink,
            > = std::sync::Arc::new(
                crate::service::tab_chat::structured_sink::ChatBackendStructuredSink::new(
                    inner,
                    &plugin_id,
                    &session_id,
                    Some(&history_meta),
                ),
            );
            // None on the first tick ⇒ the plugin returns the whole transcript and seeds the cursor.
            let mut cursor: Option<String> = None;
            let mut delay_ms = POLL_IDLE_MS;
            let mut last_signature: Option<u64> = None;
            let mut watcher_cooldowns: HashMap<String, u64> = HashMap::new();
            let mut last_watcher_verdict_ids: HashMap<String, String> = HashMap::new();
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                if svc.mux.read().get_tab(&tab_id).is_none() {
                    break;
                }
                let (svc2, plugin2, sid2, cur) = (
                    svc.clone(),
                    plugin_id.clone(),
                    session_id.clone(),
                    cursor.clone(),
                );
                let tab_id_for_tick = tab_id.clone();
                let cooldowns = watcher_cooldowns.clone();
                let verdict_ids = last_watcher_verdict_ids.clone();
                match tokio::task::spawn_blocking(
                    move || -> Result<
                        (StructuredChat, HashMap<String, u64>, HashMap<String, String>),
                        String,
                    > {
                        let mut cooldowns = cooldowns;
                        let mut verdict_ids = verdict_ids;
                        // Advance async work before reading, so a host-driven async chatBackend progresses.
                        let _ = svc2.chatbackend_pump(&plugin2, &sid2);
                        let delta = svc2.chatbackend_poll(&plugin2, &sid2, cur.as_deref())?;
                        let now_ms = epoch_ms();
                        svc2.clear_stale_watcher_inflight(&tab_id_for_tick, now_ms);
                        svc2.handle_watcher_verdict_delta(
                            &tab_id_for_tick,
                            &delta,
                            now_ms,
                            &mut cooldowns,
                            &mut verdict_ids,
                        );
                        svc2.handle_watcher_scheduler_tick(
                            &tab_id_for_tick,
                            &plugin2,
                            &sid2,
                            &delta,
                            now_ms,
                        );
                        Ok((delta, cooldowns, verdict_ids))
                    },
                )
                .await
                {
                    Ok(Ok((delta, cooldowns, verdict_ids))) => {
                        watcher_cooldowns = cooldowns;
                        last_watcher_verdict_ids = verdict_ids;
                        let tick = plan_poll_tick(&delta, cursor.take(), delay_ms, last_signature);
                        cursor = tick.cursor;
                        delay_ms = tick.delay_ms;
                        last_signature = tick.signature;
                        if tick.emit {
                            sink.emit(&tab_id, delta.messages, None);
                        }
                    }
                    Ok(Err(e)) => {
                        log::warn!("[chatbackend] poll Tab {} failed: {}", tab_id, e)
                    }
                    Err(e) => {
                        log::warn!("[chatbackend] poll task join error: {}", e);
                        break;
                    }
                }
            }
            log::info!("[chatbackend] poll loop ended for Tab {}", tab_id);
        });
    }

    fn handle_watcher_scheduler_tick(
        &self,
        tab_id: &str,
        plugin_id: &str,
        session_id: &str,
        delta: &StructuredChat,
        now_ms: u64,
    ) {
        let plugin_busy = delta.done == Some(false);
        let Some((tick, state, observations_cfg)) =
            self.prepare_due_watcher_tick(tab_id, now_ms, plugin_busy)
        else {
            return;
        };
        let observations = self.build_watcher_observation(tab_id, &observations_cfg);
        let input = tick_input_json(tick, now_ms, state, observations);
        match self.chatbackend_watcher_tick(plugin_id, session_id, input) {
            Ok(()) => {
                self.mark_watcher_tick_inflight(tab_id, tick, now_ms);
            }
            Err(e) => {
                log::warn!("[watcher] tick enqueue failed for Tab {}: {}", tab_id, e);
                self.backoff_watcher_tick(tab_id, now_ms);
                self.emit_watcher_note(tab_id, format!("Watcher tick failed to enqueue: {e}"));
            }
        }
    }

    fn handle_watcher_verdict_delta(
        &self,
        tab_id: &str,
        delta: &StructuredChat,
        now_ms: u64,
        cooldowns: &mut HashMap<String, u64>,
        last_verdict_ids: &mut HashMap<String, String>,
    ) {
        for (id, raw) in extract_watcher_verdicts(delta) {
            if !accept_watcher_verdict_id(last_verdict_ids, tab_id, &id) {
                continue;
            }
            self.apply_watcher_verdict(tab_id, &id, &raw, now_ms, cooldowns);
        }
    }

    fn clear_stale_watcher_inflight(&self, tab_id: &str, now_ms: u64) -> bool {
        let mut cleared = false;
        {
            let mut mux = self.mux.write();
            let Some(spec) = mux.get_tab_mut(tab_id).and_then(|p| p.watcher.as_mut()) else {
                return false;
            };
            if clear_stale_watcher_inflight_spec(spec, now_ms) {
                cleared = true;
            }
        }
        if cleared {
            log::warn!("[watcher] stale inflight tick cleared for Tab {}", tab_id);
            self.emit_watcher_note(tab_id, "Watcher tick timed out; scheduling will resume.");
            self.emit_state_changed();
        }
        cleared
    }

    fn prepare_due_watcher_tick(
        &self,
        tab_id: &str,
        now_ms: u64,
        plugin_busy: bool,
    ) -> Option<(u64, serde_json::Value, serde_json::Value)> {
        let mux = self.mux.read();
        let spec = mux.get_tab(tab_id)?.watcher.as_ref()?;
        match plan_watcher_tick(spec, now_ms, plugin_busy) {
            WatcherTickPlan::Arm { tick } => {
                Some((tick, spec.state.clone(), spec.observations.clone()))
            }
            WatcherTickPlan::BlockedInflight | WatcherTickPlan::Wait => None,
        }
    }

    fn mark_watcher_tick_inflight(&self, tab_id: &str, tick: u64, now_ms: u64) {
        {
            let mut mux = self.mux.write();
            if let Some(spec) = mux.get_tab_mut(tab_id).and_then(|p| p.watcher.as_mut()) {
                // One poll loop owns a tab's watcher ticks; multiple loops would need an atomic claim.
                spec.tick = tick;
                spec.tick_inflight_since_ms = Some(now_ms);
            }
        }
        self.emit_state_changed();
    }

    fn backoff_watcher_tick(&self, tab_id: &str, now_ms: u64) {
        {
            let mut mux = self.mux.write();
            if let Some(spec) = mux.get_tab_mut(tab_id).and_then(|p| p.watcher.as_mut()) {
                spec.tick = spec.tick.saturating_add(1);
                spec.last_tick_ms = Some(now_ms);
                spec.tick_inflight_since_ms = None;
            }
        }
        self.emit_state_changed();
    }

    fn build_watcher_observation(
        &self,
        tab_id: &str,
        observations_cfg: &serde_json::Value,
    ) -> serde_json::Value {
        let config = observation_config_from_json(observations_cfg);
        let snapshot = self.snapshot_with_reconciled_status();
        let orchestrator_id = collect_watch_group(&snapshot, tab_id).map(|g| g.orchestrator_id);
        let reports = orchestrator_id
            .as_deref()
            .map(|id| self.report_store.lock().get(id, false))
            .unwrap_or_default();
        let last_activity_ms = HashMap::new();
        let chat_tail = |target: &str, n: usize| -> Vec<ChatMsg> {
            crate::service::tab_chat::tab_chat_snapshot(self, target, 0)
                .map(|(mut msgs, _)| {
                    let keep_from = msgs.len().saturating_sub(n);
                    msgs.drain(..keep_from);
                    msgs
                })
                .unwrap_or_default()
        };
        let sources = ObservationSources {
            chat_tail: &chat_tail,
            reports: &reports,
            last_activity_ms: &last_activity_ms,
        };
        build_observation(&snapshot, tab_id, &config, &sources)
    }

    fn apply_watcher_verdict(
        &self,
        tab_id: &str,
        verdict_id: &str,
        raw: &str,
        now_ms: u64,
        cooldowns: &mut HashMap<String, u64>,
    ) {
        let Some((policy, previous_state, group)) = self.watcher_guardrail_context(tab_id) else {
            log::warn!(
                "[watcher] dropping late verdict {} for Tab {} with no inflight tick",
                verdict_id,
                tab_id
            );
            self.emit_watcher_note(
                tab_id,
                format!("Dropped late watcher verdict {verdict_id}; no tick is in flight."),
            );
            return;
        };
        let verdict = parse_verdict(raw);
        let outcome = apply_guardrails(
            &verdict,
            &policy,
            &group,
            cooldowns,
            now_ms,
            &previous_state,
        );
        for (dropped, reason) in &outcome.dropped {
            log::warn!(
                "[watcher] Tab {} dropped {} action to {:?}: {}",
                tab_id,
                dropped.kind,
                dropped.tab,
                reason
            );
        }
        if !outcome.dropped.is_empty() {
            let dropped_summary = outcome
                .dropped
                .iter()
                .map(|(a, r)| format!("{} -> {:?} ({r})", a.kind, a.tab))
                .collect::<Vec<_>>()
                .join(", ");
            self.emit_watcher_note(
                tab_id,
                format!("Guardrails dropped actions: {dropped_summary}"),
            );
        }
        let actions = outcome.allowed_actions.clone();
        let applied = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id)
                .and_then(|p| p.watcher.as_mut())
                .map(|spec| apply_watcher_outcome_to_spec(spec, outcome, now_ms))
                .unwrap_or(false)
        };
        if applied {
            self.execute_watcher_actions(tab_id, &actions, now_ms, cooldowns);
            self.emit_state_changed();
        } else {
            log::warn!(
                "[watcher] dropping late verdict {} for Tab {} with no inflight tick",
                verdict_id,
                tab_id
            );
            self.emit_watcher_note(
                tab_id,
                format!("Dropped late watcher verdict {verdict_id}; no tick is in flight."),
            );
        }
    }

    fn watcher_guardrail_context(
        &self,
        tab_id: &str,
    ) -> Option<(WatcherPolicy, serde_json::Value, HashSet<String>)> {
        let snapshot = self.snapshot_with_reconciled_status();
        let spec = snapshot.tabs.get(tab_id)?.watcher.as_ref()?;
        spec.tick_inflight_since_ms?;
        let group = guardrail_group(collect_watch_group(&snapshot, tab_id).ok()?);
        Some((spec.policy.clone(), spec.state.clone(), group))
    }

    fn execute_watcher_actions(
        &self,
        tab_id: &str,
        actions: &[WatcherAction],
        now_ms: u64,
        cooldowns: &mut HashMap<String, u64>,
    ) {
        execute_watcher_actions_with(
            tab_id,
            actions,
            now_ms,
            cooldowns,
            |target, message| {
                let envelope = MessageEnvelope::tab("Watcher", tab_id);
                self.gateway_deliver(
                    target,
                    envelope,
                    message,
                    crate::service::message_gateway::DeliveryClass::AgentTraffic,
                )
                .map(|_| {
                    self.emit_watcher_note(
                        tab_id,
                        format!("Nudge delivered to {target}: {message}"),
                    );
                })
            },
            |message| {
                self.create_notification(
                    "Watcher notification".to_string(),
                    Some(message.to_string()),
                    Some("warning"),
                );
            },
            |message| {
                self.submit_report(tab_id, message, ReportDetails::default(), None, None, None)
                    .map(|_| ())
            },
        );
    }

    fn emit_watcher_note(&self, tab_id: &str, text: impl Into<String>) {
        let note = watcher_system_note(text);
        if let Some((plugin_id, session_id)) = self.tab_chat_backing(tab_id) {
            crate::service::tab_chat::chatbackend_history::ChatBackendHistory::shared(
                &plugin_id,
                &session_id,
            )
            .lock()
            .upsert(std::slice::from_ref(&note));
        }
        let (source_session_id, session_binding_nonce) = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(|tab| {
                (
                    tab.agent_session_id.clone(),
                    tab.session_binding_nonce.clone(),
                )
            })
            .unwrap_or((None, None));
        crate::service::tab_chat::structured_sink::emit_chat_msgs(
            &self.app_handle,
            tab_id,
            &[note],
            None,
            source_session_id.as_deref(),
            session_binding_nonce.as_deref(),
        );
    }
}

#[cfg(test)]
mod tests;
