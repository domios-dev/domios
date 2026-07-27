//! PTY lifecycle helpers: tab env, ensure/materialize, resize, kill, resolve,
//! and wait helpers.
use super::{
    restored_agent_materialization, RestoredAgentMaterialization, SEND_AGENT_BACKEND_COLS,
    SHELL_BACKEND_COLS,
};
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::types::Tab;
use crate::service::boot_stage::BootStage;
use crate::service::plan::PlanStore;
use crate::service::ServiceContext;
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) struct RestoredLaunch {
    pub provider_id: String,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub model: Option<String>,
}

pub(crate) fn claim_restored_pty_boot(
    mux: &MuxState,
    tab_id: &str,
) -> Result<Option<RestoredLaunch>, String> {
    let mut mux = mux.write();
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
    let Some(provider_id) = tab.agent_type.clone() else {
        return Ok(None);
    };
    if !tab.restored_from_snapshot || tab.agent_booting || tab.protected {
        return Ok(None);
    }
    tab.agent_booting = true;
    Ok(Some(RestoredLaunch {
        provider_id,
        session_id: tab.agent_session_id.clone(),
        cwd: tab.cwd.clone(),
        model: tab.agent_session_model.clone(),
    }))
}

pub(crate) fn claim_restored_launch(
    mux: &MuxState,
    tab_id: &str,
    observed_provider_id: &str,
    observed_session_id: Option<&str>,
    observed_cwd: Option<&str>,
    session_valid: bool,
) -> Result<Option<RestoredLaunch>, String> {
    let mut mux = mux.write();
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found or has no PTY"))?;
    let Some(provider_id) = tab.agent_type.clone() else {
        return Ok(None);
    };
    if provider_id != observed_provider_id
        || tab.agent_session_id.as_deref() != observed_session_id
        || tab.cwd.as_deref() != observed_cwd
    {
        return Ok(None);
    }
    let Some(mode) = restored_agent_materialization(
        tab.restored_from_snapshot,
        tab.agent_booting,
        tab.protected,
        Some(&provider_id),
        tab.agent_session_id.as_deref(),
        session_valid,
    ) else {
        return Ok(None);
    };
    let session_id = match mode {
        RestoredAgentMaterialization::Resume => tab.agent_session_id.clone(),
        RestoredAgentMaterialization::Fresh => None,
    };
    if matches!(mode, RestoredAgentMaterialization::Fresh) {
        tab.agent_session_id = None;
    }
    tab.restored_from_snapshot = false;
    tab.agent_booting = true;
    Ok(Some(RestoredLaunch {
        provider_id,
        session_id,
        cwd: tab.cwd.clone(),
        model: tab.agent_session_model.clone(),
    }))
}

pub(crate) fn restore_and_schedule_launch<F>(
    mux: &MuxState,
    tab_id: &str,
    provider_id: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    session_valid: bool,
    payload: Option<String>,
    schedule: F,
) -> Result<Option<RestoredLaunch>, String>
where
    F: FnOnce(RestoredLaunch, Option<String>),
{
    let Some(launch) =
        claim_restored_launch(mux, tab_id, provider_id, session_id, cwd, session_valid)?
    else {
        return Ok(None);
    };
    schedule(launch.clone(), payload);
    Ok(Some(launch))
}

/// A restore fault is a spawn failure only when the tab has no live PTY. With
/// a live PTY the process exists, so latching SpawnFailed would lie about it.
pub(crate) fn restore_failure_boot_stage(pty_live: bool) -> Option<BootStage> {
    (!pty_live).then_some(BootStage::SpawnFailed)
}

/// Proven reattach of a live PTY must clear a stale failure; every other stage
/// already describes the tab honestly and is left alone.
pub(crate) fn reattach_boot_stage(current: Option<BootStage>) -> Option<BootStage> {
    matches!(current, Some(BootStage::SpawnFailed)).then_some(BootStage::PtyReattached)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn restore_materialization_core<R, B, L>(
    mux: &MuxState,
    tab_id: &str,
    provider_id: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    session_valid: bool,
    pty_live: bool,
    resolve_payload: R,
    boot_stage: B,
    launcher: L,
) -> Result<Option<RestoredLaunch>, String>
where
    R: FnOnce() -> Result<Option<String>, String>,
    B: FnOnce(BootStage, &str),
    L: FnOnce(RestoredLaunch, Option<String>),
{
    let payload = match resolve_payload() {
        Ok(payload) => payload,
        Err(error) => {
            if let Some(stage) = restore_failure_boot_stage(pty_live) {
                boot_stage(stage, &error);
            }
            return Err(error);
        }
    };
    restore_and_schedule_launch(
        mux,
        tab_id,
        provider_id,
        session_id,
        cwd,
        session_valid,
        payload,
        launcher,
    )
}

pub(crate) fn active_tab_lane_base(plan_store: &PlanStore, tab_id: &str) -> Option<String> {
    let (plan_id, task_id) = plan_store.task_binding_for_tab(tab_id)?;
    plan_store
        .get(&plan_id)
        .and_then(|plan| {
            plan.tasks
                .iter()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id.as_str()))
                .and_then(|task| {
                    task.execution_attempts
                        .iter()
                        .rev()
                        .find(|attempt| {
                            attempt.executor_tab_id == tab_id && attempt.closed_at.is_none()
                        })
                        .and_then(|attempt| attempt.base.clone())
                        .or_else(|| task.lane.base.clone())
                })
        })
        .filter(|base| !base.trim().is_empty())
}

pub(crate) fn tab_task_env(plan_store: &PlanStore, tab_id: &str) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    if let Some((plan_id, task_id)) = plan_store.task_binding_for_tab(tab_id) {
        if let Some(base) = active_tab_lane_base(plan_store, tab_id) {
            env_vars.insert("CODETERM_LANE_BASE".into(), base);
        }
        env_vars.insert("CODETERM_PLAN_ID".into(), plan_id);
        env_vars.insert("CODETERM_TASK_ID".into(), task_id);
    }
    env_vars
}

pub(crate) fn tab_identity_env(tab: &Tab) -> HashMap<String, String> {
    let mut env_vars = HashMap::new();
    if let Some(ref role) = tab.role {
        env_vars.insert("CODETERM_ROLE".into(), format!("{:?}", role));
    }
    if let Some(ref parent) = tab.parent_tab_id {
        env_vars.insert("CODETERM_PARENT_TAB_ID".into(), parent.clone());
    }
    if let Some(ref worker_id) = tab.worker_id {
        env_vars.insert("CODETERM_WORKER_ID".into(), worker_id.clone());
    }
    env_vars
}

impl ServiceContext {
    pub(crate) fn tab_env(&self, tab_id: &str) -> HashMap<String, String> {
        let mut env_vars = HashMap::new();
        env_vars.insert("CODETERM_TAB_ID".into(), tab_id.to_string());
        env_vars.insert(
            "CODETERM_SESSION_BINDING_NONCE".into(),
            self.ensure_session_binding_nonce(tab_id),
        );
        env_vars.insert(
            "CODETERM_SHELL_NONCE".into(),
            self.ensure_shell_integration_nonce(tab_id),
        );
        env_vars.insert(
            "CODETERM_SOCKET_PATH".into(),
            crate::socket::server::socket_display_path(),
        );
        let api_cfg = crate::config::load_config().api;
        let (api_url, api_port) = crate::config::api_callback_url(&api_cfg);
        env_vars.insert("CODETERM_API_URL".into(), api_url);
        env_vars.insert("CODETERM_API_PORT".into(), api_port);
        if let Some(ref tok) = api_cfg.token {
            env_vars.insert("CODETERM_API_TOKEN".into(), tok.clone());
        }

        let mux = self.mux.read();
        if let Some(tab) = mux.get_tab(tab_id) {
            env_vars.extend(tab_identity_env(tab));
        }
        drop(mux);
        env_vars.extend(tab_task_env(&self.plan_store, tab_id));
        env_vars
    }

    pub(crate) fn start_backend_reader_for(&self, pty_id: String) {
        let svc_reader = std::sync::Arc::new(self.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("backend reader runtime");
            rt.block_on(async {
                match crate::service::pty_lifecycle::start_backend_reader(
                    &svc_reader,
                    &pty_id,
                    true,
                ) {
                    Ok(handle) => {
                        let _ = handle.await;
                    }
                    Err(e) => {
                        log::warn!(
                            "[ensure-pty] backend reader failed for {}: {}",
                            &pty_id[..pty_id.len().min(8)],
                            e
                        );
                    }
                }
            });
        });
    }

    pub(crate) fn restored_tab_context(&self, tab_id: &str) -> Option<String> {
        let mux = self.mux.read();
        for ws in &mux.snapshot().workspaces {
            if ws.tabs.iter().any(|id| id == tab_id) {
                return Some(ws.id.clone());
            }
        }
        None
    }

    pub(crate) fn ensure_tab_pty_for_send(&self, tab_id: &str) -> Result<(), String> {
        let resolved = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
        if self.tab_chat_backing(&resolved).is_some() {
            return Ok(());
        }
        let pty_claim = self.pty_ownership.acquire(&resolved);
        self.ensure_tab_pty_for_send_inner(&resolved, &pty_claim)
    }

    fn ensure_tab_pty_for_send_inner(
        &self,
        tab_id: &str,
        pty_claim: &crate::service::pty_ownership::TabPtyClaim,
    ) -> Result<(), String> {
        if self.resolve_pty_id(tab_id).is_ok() {
            self.emit_pty_reattached(tab_id);
            self.materialize_restored_agent_for_send(tab_id)?;
            return Ok(());
        }

        let (cwd, protected, agent_type) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
            (tab.cwd.clone(), tab.protected, tab.agent_type.clone())
        };
        if protected {
            return Err(format!(
                "Tab {} is protected and its backend PTY is not live yet",
                tab_id
            ));
        }

        let env_vars = self.tab_env(tab_id);
        let cols = if agent_type.is_some() {
            SEND_AGENT_BACKEND_COLS
        } else {
            SHELL_BACKEND_COLS
        };
        let t0 = std::time::Instant::now();
        let spawn_gate = self.pty.lock().spawn_gate();
        let built =
            crate::pty::manager::build_session(spawn_gate, None, cwd.clone(), cols, 40, env_vars)?;
        let (pty_id, _) = self.pty.lock().register_session(None, built);
        self.screen_state.lock().resize(&pty_id, cols, 40);
        log::info!(
            "[ensure-pty] Tab {} activated backend pty {} (build={}ms)",
            tab_id,
            &pty_id[..pty_id.len().min(8)],
            t0.elapsed().as_millis()
        );
        let selection = self.link_pty_with_claim(pty_claim, tab_id, pty_id, false);
        if !selection.linked {
            return Err(format!("Tab {} closed before PTY link", tab_id));
        }
        if let Some(reader_pty_id) = selection.reader_pty_id() {
            self.start_backend_reader_for(reader_pty_id.to_string());
        }

        self.materialize_restored_agent_for_send(tab_id)?;
        Ok(())
    }

    fn emit_pty_reattached(&self, tab_id: &str) {
        let current = self.mux.read().get_tab(tab_id).and_then(|p| p.boot_stage);
        if let Some(stage) = reattach_boot_stage(current) {
            self.emit_boot_stage(tab_id, stage, None);
        }
    }

    pub(crate) fn materialize_restored_agent_for_send(&self, tab_id: &str) -> Result<(), String> {
        let (provider_id, session_id, cwd) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
            (
                tab.agent_type.clone(),
                tab.agent_session_id.clone(),
                tab.cwd.clone(),
            )
        };
        let Some(provider_id) = provider_id else {
            return Ok(());
        };
        let session_valid = session_id
            .as_deref()
            .zip(cwd.as_deref())
            .map(|(sid, dir)| {
                self.agent_registry
                    .with_provider(&provider_id, |p| p.session_exists(dir, sid))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let _workspace_id = self.restored_tab_context(tab_id).ok_or_else(|| {
            format!(
                "Tab {} is a restored agent Tab but has no tab context for materialization",
                tab_id
            )
        })?;
        let scheduled = restore_materialization_core(
            &self.mux,
            tab_id,
            &provider_id,
            session_id.as_deref(),
            cwd.as_deref(),
            session_valid,
            self.resolve_pty_id(tab_id).is_ok(),
            || {
                self.resolve_pending_delivery(tab_id)
                    .map_err(|error| error.to_string())
            },
            |stage, error| self.emit_boot_stage(tab_id, stage, Some(error)),
            |launch, restored_task| {
                let pc = crate::config::provider_config(&launch.provider_id);
                let params = crate::service::SpawnAgentParams {
                    agent: Some(launch.provider_id.clone()),
                    cwd: launch.cwd,
                    skip_permissions: Some(pc.resume_skip_permissions.unwrap_or(true)),
                    model: launch.model,
                    resume_session_id: launch.session_id,
                    ..Default::default()
                };
                let launch_cmd = crate::service::orchestrator::build_agent_command(
                    &launch.provider_id,
                    &params,
                    None,
                    None,
                    &self.agent_registry,
                );
                crate::service::orchestrator::launch_agent_in_background(
                    self.clone(),
                    tab_id.to_string(),
                    launch_cmd,
                    launch.provider_id,
                    restored_task,
                    None,
                    None,
                );
            },
        );
        if scheduled?.is_none() {
            return Ok(());
        }
        self.emit_state_changed();
        Ok(())
    }

    pub fn resize_tab(&self, tab_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        // A chatBackend tab has no terminal to resize — graceful no-op.
        if self.tab_chat_backing(tab_id).is_some() {
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(tab_id)?;
        self.pty
            .lock()
            .resize(&pty_id, cols, rows)
            .map_err(|e| e.to_string())?;
        self.screen_state.lock().resize(&pty_id, cols, rows);
        crate::pty::capture::CaptureRegistry::global().tap_resize(&pty_id, cols, rows);
        Ok(())
    }

    /// Kill the PTY process for a tab without removing the tab from the mux.
    /// Different from `close_tab`, which both kills the PTY and removes the tab.
    pub fn kill_tab_pty(&self, tab_id: &str) -> Result<(), String> {
        // A chatBackend tab has no PTY process — close the plugin session.
        if let Some((plugin_id, session_id)) = self.tab_chat_backing(tab_id) {
            self.chatbackend_close(&plugin_id, &session_id)?;
            self.clear_tab_agent_identity_for_lifecycle_exit(tab_id);
            return Ok(());
        }
        let pty_id = self.resolve_pty_id(tab_id)?;
        self.pty_ownership.release_pty(&pty_id);
        kill_live_tab_pty(&mut self.pty.lock(), &pty_id)?;
        self.clear_tab_agent_identity_for_lifecycle_exit(tab_id);
        Ok(())
    }

    /// Resolve tab_id (or alias) → pty_id.
    pub fn resolve_pty_id(&self, tab_id: &str) -> Result<String, String> {
        let mux = self.mux.read();
        let resolved = mux
            .resolve_tab_id(tab_id)
            .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
        let pty_id = mux
            .get_tab(&resolved)
            .and_then(|p| p.pty_id.clone())
            .ok_or_else(|| format!("Tab {} not found or has no PTY", tab_id))?;
        drop(mux);
        if self.pty.lock().get_session(&pty_id).is_none() {
            return Err(format!("Tab {} PTY is not live: {}", tab_id, pty_id));
        }
        Ok(pty_id)
    }

    /// Wait for a tab to get a PTY assigned by the frontend. Polls with timeout.
    pub async fn wait_for_pty(&self, tab_id: &str, timeout_secs: u64) -> Result<String, String> {
        let mux = self.mux.clone();
        let pid = tab_id.to_string();
        tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async {
            loop {
                {
                    let m = mux.read();
                    if let Some(tab) = m.get_tab(&pid) {
                        if let Some(ref pty_id) = tab.pty_id {
                            return pty_id.clone();
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
            }
        })
        .await
        .map_err(|_| "Timeout waiting for PTY".to_string())
    }

    /// Wait for output buffer to grow by at least `min_bytes`.
    pub async fn wait_for_output_growth(&self, pty_id: &str, min_bytes: usize, timeout_secs: u64) {
        let ob = self.output_buf.clone();
        let pid = pty_id.to_string();
        let _ = tokio::time::timeout(tokio::time::Duration::from_secs(timeout_secs), async move {
            let start_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
            loop {
                let cur_len = ob.lock().get(&pid).map(|b| b.len()).unwrap_or(0);
                if cur_len > start_len + min_bytes {
                    return;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        })
        .await;
    }
}

pub(crate) fn kill_live_tab_pty(
    manager: &mut crate::pty::manager::PtyManager,
    pty_id: &str,
) -> Result<(), String> {
    manager.kill(pty_id)
}

#[cfg(test)]
mod restore_boot_stage_tests {
    use super::{reattach_boot_stage, restore_failure_boot_stage, restore_materialization_core};
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{PendingDeliveryChannel, PendingDeliveryRecord};
    use crate::service::boot_stage::{apply_boot_stage, BootStage};
    use codeterm_util::lock_hold::TimedRwLock;
    use sha2::Digest;
    use std::sync::Arc;

    struct RestoreFixture {
        _temp: tempfile::TempDir,
        store: crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore,
        mux: MuxState,
        tab_id: String,
        digest: String,
    }

    /// A restored agent tab whose pending-delivery payload is absent from the
    /// store, so the resolve step is guaranteed to fail.
    fn unresolvable_restored_tab(live_pty: bool) -> RestoreFixture {
        let digest = format!(
            "{:x}",
            sha2::Sha256::digest("missing restored fallback brief".as_bytes())
        );
        let temp = tempfile::tempdir().unwrap();
        let store = crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
            temp.path(),
            &std::collections::HashSet::new(),
        )
        .unwrap();
        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.cwd = Some("fixture-cwd".into());
        tab.restored_from_snapshot = true;
        tab.pty_id = live_pty.then(|| "live-pty".to_string());
        tab.pending_delivery = Some(PendingDeliveryRecord::new(
            digest.clone(),
            PendingDeliveryChannel::InteractiveInput,
        ));
        RestoreFixture {
            _temp: temp,
            store,
            mux: Arc::new(TimedRwLock::new("restore-boot-stage-test", state)),
            tab_id,
            digest,
        }
    }

    fn restore_with_failing_resolve(
        fixture: &RestoreFixture,
        pty_live: bool,
    ) -> (Result<(), String>, Vec<(BootStage, String)>, bool) {
        let emitted = parking_lot::Mutex::new(Vec::new());
        let launched = parking_lot::Mutex::new(false);
        let result = restore_materialization_core(
            &fixture.mux,
            &fixture.tab_id,
            "fixture-provider",
            None,
            Some("fixture-cwd"),
            false,
            pty_live,
            || {
                fixture
                    .store
                    .resolve(&fixture.digest)
                    .map(Some)
                    .map_err(|error| error.to_string())
            },
            |stage, reason| emitted.lock().push((stage, reason.to_string())),
            |_, _| *launched.lock() = true,
        );
        let stages = emitted.lock().clone();
        let launched = *launched.lock();
        (result.map(|_| ()), stages, launched)
    }

    #[test]
    fn restore_binding_failure_on_live_pty_never_latches_spawn_failed() {
        let fixture = unresolvable_restored_tab(true);
        let (result, stages, launched) = restore_with_failing_resolve(&fixture, true);

        assert_eq!(
            result.unwrap_err(),
            format!("RestorePayloadMissing: {}", fixture.digest),
            "the resolve error must still surface to the caller"
        );
        assert!(
            stages.is_empty(),
            "a binding failure over a live PTY must emit no boot-stage failure: {stages:?}"
        );
        assert!(!launched, "a failed resolve must not launch an agent");
    }

    #[test]
    fn restore_failure_without_live_pty_still_reports_spawn_failed() {
        let fixture = unresolvable_restored_tab(false);
        let (result, stages, launched) = restore_with_failing_resolve(&fixture, false);

        assert!(result.is_err());
        assert_eq!(
            stages,
            vec![(
                BootStage::SpawnFailed,
                format!("RestorePayloadMissing: {}", fixture.digest)
            )],
            "with no live PTY a real spawn failure must stay distinguishable"
        );
        assert!(!launched);
    }

    #[test]
    fn proven_reattach_clears_a_latched_failure_without_restart() {
        let (mut state, tab_id) = MultiplexerState::new();
        let tab = state.get_tab_mut(&tab_id).unwrap();
        let mut sequence = Vec::new();

        apply_boot_stage(
            tab,
            BootStage::SpawnFailed,
            Some("RestorePayloadMissing: deadbeef"),
        );
        sequence.push(tab.boot_stage.unwrap());
        assert_eq!(
            tab.boot_failure_reason.as_deref(),
            Some("RestorePayloadMissing: deadbeef"),
            "precondition: the Tab starts with a latched failure"
        );

        let reattached = reattach_boot_stage(tab.boot_stage).expect("reattach clears the latch");
        apply_boot_stage(tab, reattached, None);
        sequence.push(tab.boot_stage.unwrap());

        assert_eq!(
            sequence,
            vec![BootStage::SpawnFailed, BootStage::PtyReattached],
            "the UI-visible stage must transition out of the failure on reattach"
        );
        assert_eq!(
            tab.boot_failure_reason, None,
            "the latched failure reason must be cleared, not merely overdrawn"
        );
        assert!(
            reattach_boot_stage(tab.boot_stage).is_none(),
            "reattach is idempotent"
        );
    }

    #[test]
    fn reattach_never_rewrites_an_already_honest_stage() {
        for stage in [
            None,
            Some(BootStage::AgentReady),
            Some(BootStage::ShellReady),
            Some(BootStage::AgentLaunching),
            Some(BootStage::PtyReattached),
        ] {
            assert_eq!(
                reattach_boot_stage(stage),
                None,
                "reattach must not stomp an in-flight or ready boot: {stage:?}"
            );
        }
    }

    #[test]
    fn spawn_failure_taxonomy_keys_off_pty_liveness() {
        assert_eq!(
            restore_failure_boot_stage(false),
            Some(BootStage::SpawnFailed)
        );
        assert_eq!(restore_failure_boot_stage(true), None);
    }
}

#[cfg(test)]
mod kill_tab_pty_tests {
    use std::collections::HashMap;

    use super::kill_live_tab_pty;

    #[test]
    fn service_kill_tab_pty_helper_uses_kill_semantics() {
        let mut manager = crate::pty::manager::PtyManager::new();
        let (pty_id, session) = manager
            .spawn(None, None, 80, 24, HashMap::new())
            .expect("spawn default shell");

        kill_live_tab_pty(&mut manager, &pty_id).expect("kill succeeds");

        assert!(
            manager.get_session(&pty_id).is_none(),
            "service PTY kill must remove the manager entry"
        );
        // TerminateProcess is async on Windows and the reaper thread may reap
        // concurrently; poll until the child is gone (here or in background).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match session.child.lock().expect("child lock").try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "service PTY kill must terminate/reap the child path via PtyManager::kill"
            );
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}
