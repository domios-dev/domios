use std::collections::HashSet;

use super::{finish_run, AgentRunKind, Ordering, RunWatcherRegistry};
use crate::agent::chat_types::ChatMsg;
use crate::service::role_policy::CronSettlement;
use crate::service::ServiceContext;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Default)]
pub struct CompletionBarrier {
    pub stopped: bool,
    pub assistant_ids: HashSet<String>,
}

impl CompletionBarrier {
    pub(super) fn ready(&self, messages: &[ChatMsg]) -> bool {
        self.stopped
            && messages
                .iter()
                .rev()
                .find(|message| {
                    matches!(
                        message.kind.as_str(),
                        "assistant" | "user" | "tool" | "tool_call" | "tool_use" | "tool_result"
                    )
                })
                .is_some_and(|message| {
                    message.kind == "assistant"
                        && !message.content.trim().is_empty()
                        && !self.assistant_ids.contains(&message.id)
                })
    }
}

pub struct PendingCompletion {
    pub barrier: CompletionBarrier,
    outcome: CronSettlement,
    provider: String,
    cwd: String,
    session: String,
    nonce: String,
    pub _watcher: Option<RecommendedWatcher>,
    #[cfg(any(test, feature = "test-support"))]
    pub events: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    #[cfg(any(test, feature = "test-support"))]
    pub source_path: Option<std::path::PathBuf>,
}

impl RunWatcherRegistry {
    pub fn ensure_cron_watcher(
        &self,
        svc: &ServiceContext,
        tab: &str,
        run: &str,
    ) -> Result<(), String> {
        self.cron_completion_state(svc, tab, run).map(|_| ())
    }

    pub fn request_cron_completion(
        &self,
        svc: &ServiceContext,
        tab_id: &str,
        run_id: &str,
        outcome: &CronSettlement,
    ) -> Result<(), String> {
        if let Some(result) = self.request_runtime_cron_completion(svc, tab_id, run_id, outcome) {
            return result;
        }
        let state = self.cron_completion_state(svc, tab_id, run_id)?;
        if let Some(pending) = state.cron_completion.lock().as_ref() {
            return pending.outcome.confirm_outcome(outcome);
        }
        let (provider, cwd, session, nonce) = {
            let mux = svc.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("cron tab closed")?;
            if tab.pty_id.as_deref() != Some(state.pty_id.as_str())
                || tab.session_binding_nonce != state.binding_nonce
            {
                return Err("cron completion launch changed".into());
            }
            (
                tab.agent_type.clone().ok_or("cron provider missing")?,
                tab.cwd.clone().unwrap_or_default(),
                tab.agent_session_id.clone().ok_or("cron session missing")?,
                tab.session_binding_nonce
                    .clone()
                    .ok_or("cron binding missing")?,
            )
        };
        let messages = if matches!(&state.kind, AgentRunKind::StepRun { .. }) {
            vec![]
        } else {
            crate::service::tab_chat::session_chat_snapshot(svc, &provider, &cwd, &session, 0)?.0
        };
        let path = svc
            .agent_registry
            .with_provider(&provider, |p| p.session_file_path(&cwd, &session))
            .flatten();
        #[cfg(any(test, feature = "test-support"))]
        let source_path = path.clone();
        let mut watcher = None;
        #[cfg(any(test, feature = "test-support"))]
        let events = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        if let Some(path) = path {
            let observed = path.clone();
            let service = svc.clone();
            let tab = tab_id.to_string();
            #[cfg(any(test, feature = "test-support"))]
            let recorded_events = events.clone();
            let mut source_watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    log::debug!(
                        "[cron] transcript event tab={tab} path={observed:?} event={event:?}"
                    );
                    #[cfg(any(test, feature = "test-support"))]
                    {
                        let mut events = recorded_events.lock();
                        if events.len() < 16 {
                            events.push(format!("{event:?}"));
                        }
                    }
                    if event.is_ok_and(|event| {
                        matches!(
                            event.kind,
                            notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                        ) && event.paths.iter().any(|path| path == &observed)
                    }) {
                        service.run_watcher.check_pending_cron(&service, &tab);
                    }
                })
                .map_err(|error| error.to_string())?;
            source_watcher
                .watch(
                    path.parent().ok_or("cron transcript has no parent")?,
                    RecursiveMode::NonRecursive,
                )
                .map_err(|error| error.to_string())?;
            watcher = Some(source_watcher);
        }
        let mux = svc.mux.read();
        let cron = svc.try_ext::<crate::CronState>().map(|cron| cron.lock());
        if !mux.get_tab(tab_id).is_some_and(|tab| {
            tab.pty_id.as_deref() == Some(state.pty_id.as_str())
                && tab.session_binding_nonce == state.binding_nonce
        }) || !self.pty_cron_registration_allowed(cron.as_deref(), &mux, &state)
            || !self
                .runs
                .lock()
                .get(&state.pty_id)
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, &state))
        {
            return Err("cron completion lost its original watcher".into());
        }
        let mut slot = state.cron_completion.lock();
        if let Some(pending) = slot.as_ref() {
            return pending.outcome.confirm_outcome(outcome);
        }
        if state.completed.load(Ordering::Acquire) {
            return Err("Cron watcher already claimed completion".into());
        }
        *slot = Some(PendingCompletion {
            barrier: CompletionBarrier {
                stopped: false,
                assistant_ids: if matches!(&state.kind, AgentRunKind::StepRun { .. }) {
                    Default::default()
                } else {
                    messages.iter().map(|message| message.id.clone()).collect()
                },
            },
            outcome: outcome.clone(),
            provider,
            cwd,
            session,
            nonce,
            _watcher: watcher,
            #[cfg(any(test, feature = "test-support"))]
            events,
            #[cfg(any(test, feature = "test-support"))]
            source_path,
        });
        drop(slot);
        drop(cron);
        drop(mux);
        log::info!("[cron] completion requested; awaiting completed turn and durable answer: tab={tab_id} run={run_id}");
        Ok(())
    }

    fn cron_completion_state(
        &self,
        svc: &ServiceContext,
        tab_id: &str,
        run_id: &str,
    ) -> Result<std::sync::Arc<super::RunState>, String> {
        {
            let mux = svc.mux.read();
            let runs = self.runs.lock();
            if let Some(state) = runs.values().find(|state| {
                state.tab_id == tab_id
                    && matches!(&state.kind, AgentRunKind::StepRun { run_id: own } if own == run_id)
            }) {
                if !mux.get_tab(tab_id).is_some_and(|tab| {
                    tab.pty_id.as_deref() == Some(state.pty_id.as_str())
                        && tab.session_binding_nonce == state.binding_nonce
                }) {
                    return Err("step run launch ownership changed".into());
                }
                return Ok(state.clone());
            }
        }
        let (owner, timeout, existing) = self.cron_pty_authority(svc, tab_id, run_id)?;
        if let Some(existing) = existing {
            return Ok(existing);
        }
        self.register_cron(svc.clone(), &owner, timeout, None)?;
        let (current, _, registered) = self.cron_pty_authority(svc, tab_id, run_id)?;
        if current != owner {
            return Err("cron launch changed during watcher registration".into());
        }
        registered.ok_or_else(|| "cron watcher registration ended".into())
    }

    fn cron_pty_authority(
        &self,
        svc: &ServiceContext,
        tab: &str,
        run: &str,
    ) -> Result<
        (
            super::runtime_runs::CronRunAuthority,
            u64,
            Option<std::sync::Arc<super::RunState>>,
        ),
        String,
    > {
        let fallback = svc.configuration().memory.worker_timeout_seconds;
        let mux = svc.mux.read();
        let cron = svc
            .try_ext::<crate::CronState>()
            .ok_or("cron unavailable")?
            .lock();
        let (job, owned_run) = cron
            .get_tab_owner(tab)
            .ok_or("cron tab has no active run")?;
        if owned_run != run {
            return Err("cron run ownership changed".into());
        }
        if !cron.run_is_active(job, run) {
            return Err("cron run already ended".into());
        }
        let owner = self
            .cron_binding(tab, job, run)
            .ok_or("cron launch has no original witness")?;
        if !owner.matches(&mux) {
            return Err("cron launch generation changed".into());
        }
        let pty = owner
            .pty_id
            .as_ref()
            .ok_or("cron launch has no owned PTY")?;
        let runs = self.runs.lock();
        let state = match runs.get(pty) {
            Some(state)
                if state.tab_id == tab
                    && state.binding_nonce == owner.binding_nonce
                    && state.kind
                        == (AgentRunKind::Cron {
                            job_id: job.clone(),
                            run_id: run.into(),
                        }) =>
            {
                Some(state.clone())
            }
            Some(_) => return Err("cron PTY watcher belongs to another launch".into()),
            None => None,
        };
        let timeout = cron
            .get(job)
            .and_then(|job| job.timeout_seconds)
            .unwrap_or(fallback);
        Ok((owner, timeout, state))
    }
    pub(super) fn stop_pending_cron(&self, svc: &ServiceContext, tab_id: &str) -> bool {
        let state = self
            .runs
            .lock()
            .values()
            .find(|state| state.tab_id == tab_id)
            .cloned();
        let Some(state) = state else {
            return false;
        };
        if let AgentRunKind::StepRun { run_id } = &state.kind {
            if state.cron_completion.lock().is_none() {
                if let Err(error) = self.request_cron_completion(
                    svc,
                    tab_id,
                    run_id,
                    &CronSettlement::success("agent turn completed"),
                ) {
                    self.fail_now(svc, &state.pty_id, error);
                    return true;
                }
            }
        }
        {
            let mut pending = state.cron_completion.lock();
            let Some(pending) = pending.as_mut() else {
                return false;
            };
            pending.barrier.stopped = true;
        }
        self.check_pending_cron(svc, tab_id);
        true
    }

    pub(crate) fn check_pending_cron(&self, svc: &ServiceContext, tab_id: &str) {
        let state = self
            .runs
            .lock()
            .values()
            .find(|state| state.tab_id == tab_id)
            .cloned();
        let Some(state) = state else {
            return;
        };
        let scope = {
            let pending = state.cron_completion.lock();
            let Some(pending) = pending.as_ref().filter(|pending| pending.barrier.stopped) else {
                return;
            };
            (
                pending.provider.clone(),
                pending.cwd.clone(),
                pending.session.clone(),
                pending.nonce.clone(),
            )
        };
        let service = svc.clone();
        crate::async_runtime::spawn(async move {
            let messages = {
                let reader = service.clone();
                let (provider, cwd, session) = (scope.0.clone(), scope.1.clone(), scope.2.clone());
                match crate::async_runtime::spawn_blocking(move || {
                    crate::service::tab_chat::session_chat_snapshot(
                        &reader, &provider, &cwd, &session, 0,
                    )
                })
                .await
                {
                    Ok(Ok((messages, _))) => messages,
                    error => {
                        log::debug!(
                            "[cron] pending transcript read failed tab={} result={error:?}",
                            state.tab_id
                        );
                        return;
                    }
                }
            };
            let current = service
                .mux
                .read()
                .get_tab(&state.tab_id)
                .is_some_and(|tab| {
                    tab.pty_id.as_deref() == Some(state.pty_id.as_str())
                        && tab.session_binding_nonce == state.binding_nonce
                        && tab.agent_session_id.as_deref() == Some(&scope.2)
                        && tab.session_binding_nonce.as_deref() == Some(&scope.3)
                });
            if !current {
                log::debug!(
                    "[cron] pending completion binding changed tab={}",
                    state.tab_id
                );
                return;
            }
            let outcome = {
                let pending = state.cron_completion.lock();
                let Some(pending) = pending
                    .as_ref()
                    .filter(|pending| pending.barrier.ready(&messages))
                else {
                    log::debug!(
                        "[cron] final-answer barrier held tab={} messages={}",
                        state.tab_id,
                        messages.len()
                    );
                    return;
                };
                pending.outcome.clone()
            };
            if state.completed.swap(true, Ordering::AcqRel) {
                return;
            }
            log::info!(
                "[cron] completed turn and durable final answer confirmed: tab={}",
                state.tab_id
            );
            let finisher = service.clone();
            let ending = state.clone();
            let _ = crate::async_runtime::spawn_blocking(move || {
                finish_run(&finisher, &ending, outcome.success, outcome.summary)
            })
            .await;
            service.run_watcher.retire_finished_watcher(&state);
        });
    }
}

#[cfg(test)]
mod tests;
