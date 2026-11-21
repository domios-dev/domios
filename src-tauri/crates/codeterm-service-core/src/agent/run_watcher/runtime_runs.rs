use super::{cron_end, AgentRunKind, RunWatcherRegistry};
use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::TabCloseReason;
use crate::service::agent_runtime::engine;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::RuntimeContext;
use std::time::Duration;

pub(super) struct RuntimeRun {
    nonce: String,
    kind: AgentRunKind,
    armed: bool,
    ended_turn: bool,
    completion: Option<RuntimeCompletion>,
    reported: bool,
    cancelled: tokio::sync::watch::Sender<bool>,
}

struct RuntimeCompletion {
    summary: String,
    barrier: super::cron_completion::CompletionBarrier,
}

struct RuntimeRunKey<'a> {
    nonce: &'a str,
    kind: Option<&'a AgentRunKind>,
    failed_start_epoch: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CronRunAuthority {
    pub(super) tab_id: String,
    pub(super) job_id: String,
    pub(super) run_id: String,
    pub(super) binding_nonce: Option<String>,
    pub(super) pty_id: Option<String>,
}

pub(super) struct CronCleanup {
    owner: CronRunAuthority,
    pty: Option<std::sync::Arc<crate::pty::manager::PtySession>>,
}

impl CronRunAuthority {
    pub(crate) fn attach_claimed_pty(&self, mux: &crate::multiplexer::state::MultiplexerState, selected: &crate::service::pty_ownership::ClaimedPty<'_>) -> Result<Self, String> {
        if self.pty_id.is_some() || !self.matches(mux) || selected.tab_id() != self.tab_id { return Err("Cron original launch changed before PTY attachment".into()); }
        Ok(Self { pty_id: Some(selected.pty_id().into()), ..self.clone() })
    }
    pub(crate) fn tab_id(&self) -> &str { &self.tab_id }
    pub(crate) fn job_id(&self) -> &str { &self.job_id }
    pub(crate) fn run_id(&self) -> &str { &self.run_id }
    pub(crate) fn nonce(&self) -> Option<&str> { self.binding_nonce.as_deref() }
    pub(crate) fn pty_id(&self) -> Option<&str> { self.pty_id.as_deref() }
    pub(crate) fn matches(&self, mux: &crate::multiplexer::state::MultiplexerState) -> bool {
        mux.get_tab(&self.tab_id).is_some_and(|tab| {
            tab.session_binding_nonce == self.binding_nonce && tab.pty_id == self.pty_id
        })
    }
}

impl RunWatcherRegistry {
    pub(super) fn retire_finished_watcher(&self, state: &super::RunState) {
        if matches!(state.kind, AgentRunKind::Cron { .. }) { return; }
        let mut runs = self.runs.lock();
        if runs.get(&state.pty_id).is_some_and(|current| std::ptr::eq(current.as_ref(), state)) { runs.remove(&state.pty_id); }
    }

    pub(crate) fn register_cron(&self, svc: ServiceContext, owner: &CronRunAuthority, timeout: u64, resume: Option<String>) -> Result<(), String> {
        self.register_owned(svc, owner.pty_id.clone().ok_or("Cron launch has no PTY")?, owner.tab_id.clone(), AgentRunKind::Cron { job_id: owner.job_id.clone(), run_id: owner.run_id.clone() }, timeout, resume, Some(owner)).map(|_| ())
    }

    pub(super) fn original_cron_registration_allowed(&self, owner: Option<&CronRunAuthority>, mux: &crate::multiplexer::state::MultiplexerState, state: &super::RunState) -> bool {
        !matches!(state.kind, AgentRunKind::Cron { .. }) || owner.is_some_and(|owner| {
            owner.tab_id == state.tab_id && owner.pty_id.as_deref() == Some(state.pty_id.as_str())
                && owner.binding_nonce == state.binding_nonce && owner.matches(mux)
                && self.cron_bindings.lock().get(&state.tab_id) == Some(owner)
        })
    }

    pub(super) fn pty_cron_registration_allowed(&self, store: Option<&crate::cron::store::CronStore>, mux: &crate::multiplexer::state::MultiplexerState, state: &super::RunState) -> bool {
        let AgentRunKind::Cron { job_id, run_id } = &state.kind else { return true };
        store.is_some_and(|store| store.run_is_active(job_id, run_id))
            && self.cron_bindings.lock().get(&state.tab_id).is_some_and(|owner| {
                owner.job_id == *job_id && owner.run_id == *run_id && owner.pty_id.as_deref() == Some(state.pty_id.as_str())
                    && owner.binding_nonce == state.binding_nonce && owner.matches(mux)
            })
    }

    pub(crate) fn capture_cron_launch(&self, svc: &ServiceContext, linkage: &crate::service::spawn::RuntimeOwnerLinkage, tab: &str, nonce: &str, pty: Option<&str>) -> Result<CronRunAuthority, String> {
        if pty.is_some() { return Err("Cron PTY authority requires the original claim attachment".into()); }
        let crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id } = linkage else { return Err("Launch is not a cron run".into()) };
        let owner = CronRunAuthority {
            tab_id: tab.into(), job_id: job_id.clone(), run_id: run_id.clone(), binding_nonce: Some(nonce.into()), pty_id: pty.map(str::to_string),
        };
        let mux = svc.mux.read();
        if !owner.matches(&mux) { return Err("Cron launch changed before reservation".into()); }
        Ok(owner)
    }

    pub(crate) fn runtime_start_owner(&self, tab: &str, nonce: &str) -> Option<AgentRunKind> {
        self.runtime_runs.lock().get(tab).filter(|run| run.nonce == nonce).map(|run| run.kind.clone())
    }

    pub(crate) fn runtime_start_result(
        &self,
        svc: &ServiceContext,
        tab: &str,
        nonce: &str,
        kind: &AgentRunKind,
        error: &crate::service::agent_runtime::model::RuntimeFailure,
    ) -> Option<cron_end::CronEndHandle> {
        use crate::service::agent_runtime::model::RuntimeFailure;
        if matches!(error, RuntimeFailure::Superseded | RuntimeFailure::SignInRequired | RuntimeFailure::WrittenAwaitingEvidence) {
            return None;
        }
        let failed_start_epoch = if matches!(error, RuntimeFailure::Deadline) { Some(ended_unbound_start(svc, tab, nonce)?) } else { None };
        let current = svc.mux.read().get_tab(tab).is_some_and(|target| {
            target.session_binding_nonce.as_deref() == Some(nonce)
        });
        if !current {
            return None;
        }
        self.finish_runtime_owned(svc, tab, RuntimeRunKey { nonce, kind: Some(kind), failed_start_epoch }, false, false, format!("Runtime startup failed: {error}"))
    }

    pub(crate) fn has_runtime_run(&self, tab: &str) -> bool {
        self.runtime_runs.lock().contains_key(tab)
    }

    pub(crate) fn register_runtime(
        &self,
        svc: &ServiceContext,
        tab: &str,
        kind: AgentRunKind,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let mux = svc.mux.read();
        let nonce = {
            let target = mux.get_tab(tab).ok_or("Runtime run tab absent")?;
            if !target
                .agent_type
                .as_deref()
                .is_some_and(|provider| engine::requested(svc, provider))
            {
                return Err("Runtime run requires agentRuntime".into());
            }
            target
                .session_binding_nonce
                .clone()
                .ok_or("Runtime run binding absent")?
        };
        let (cancelled, mut completion) = tokio::sync::watch::channel(false);
        {
            let cron = svc.try_ext::<crate::CronState>().map(|cron| cron.lock());
            if let AgentRunKind::Cron { job_id, run_id } = &kind {
                let active = cron.as_ref().is_some_and(|store| {
                    store.run_is_active(job_id, run_id)
                        && store.run_receipt(job_id, run_id).is_some_and(|run| run.tab_id.as_deref() == Some(tab))
                        && self.cron_binding(tab, job_id, run_id).is_some_and(|owner| owner.binding_nonce.as_deref() == Some(nonce.as_str()) && owner.pty_id.is_none() && owner.matches(&mux))
                });
                if !active {
                    return Err("Runtime cron run is no longer active".into());
                }
            }
            let mut runs = self.runtime_runs.lock();
            if let Some(existing) = runs.get(tab) {
                return if existing.nonce == nonce && existing.kind == kind {
                    Ok(())
                } else {
                    Err("Runtime run ownership changed".into())
                };
            }
            runs.insert(
                tab.into(),
                RuntimeRun {
                    nonce: nonce.clone(),
                    kind: kind.clone(),
                    armed: false,
                    ended_turn: false,
                    completion: matches!(&kind, AgentRunKind::StepRun { .. }).then(|| {
                        RuntimeCompletion {
                            summary: "agent turn completed".into(),
                            barrier: super::cron_completion::CompletionBarrier::default(),
                        }
                    }),
                    reported: false,
                    cancelled,
                },
            );
        }
        drop(mux);
        {
            let owner = svc.clone();
            let tab = tab.to_string();
            crate::async_runtime::spawn(async move {
                if runtime_deadline(&mut completion, tokio::time::sleep(Duration::from_secs(timeout_secs))).await {
                    let watcher = owner.run_watcher.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = watcher.finish_runtime_owned(
                            &owner,
                            &tab,
                            RuntimeRunKey { nonce: &nonce, kind: Some(&kind), failed_start_epoch: None },
                            false,
                            true,
                            format!("hard timeout after {timeout_secs}s"),
                        );
                    })
                    .await;
                }
            });
        }
        Ok(())
    }

    pub(crate) fn arm_runtime(&self, tab: &str, nonce: &str) {
        if let Some(run) = self
            .runtime_runs
            .lock()
            .get_mut(tab)
            .filter(|run| run.nonce == nonce)
        {
            run.armed = true;
        }
    }

    pub fn request_runtime_cron_completion(
        &self,
        svc: &ServiceContext,
        tab: &str,
        run_id: &str,
        summary: &str,
    ) -> Option<Result<(), String>> {
        if self.resume_cron_finalizer(svc, tab, run_id) { return Some(Ok(())); }
        let messages = runtime_messages(tab);
        let nonce = svc
            .mux
            .read()
            .get_tab(tab)
            .and_then(|target| target.session_binding_nonce.clone());
        let mut runs = self.runtime_runs.lock();
        let run = runs.get_mut(tab)?;
        if !matches!(&run.kind, AgentRunKind::Cron { run_id: own, .. } if own == run_id) {
            return Some(Err("Runtime cron run ownership changed".into()));
        }
        if nonce.as_deref() != Some(run.nonce.as_str()) {
            return Some(Err("Runtime cron binding changed".into()));
        }
        run.completion.get_or_insert_with(|| RuntimeCompletion {
            summary: summary.into(),
            barrier: super::cron_completion::CompletionBarrier {
                stopped: false,
                assistant_ids: messages.iter().map(|message| message.id.clone()).collect(),
            },
        });
        Some(Ok(()))
    }

    pub fn runtime_worker_reported(&self, tab: &str, worker: &str) -> bool {
        let mut runs = self.runtime_runs.lock();
        let Some(run) = runs.get_mut(tab).filter(|run| {
            matches!(&run.kind,
            AgentRunKind::MemWorker { worker_id } if worker_id == worker)
        }) else {
            return false;
        };
        run.reported = true;
        let _ = run.cancelled.send(true);
        true
    }

    pub(crate) fn runtime_turn_completed(&self, svc: &ServiceContext, context: &RuntimeContext) {
        if !current_runtime_binding(svc, context) {
            return;
        }
        {
            let mut runs = self.runtime_runs.lock();
            let Some(run) = runs
                .get_mut(&context.tab_id)
                .filter(|run| run.nonce == context.binding_nonce && run.armed)
            else {
                return;
            };
            run.ended_turn = true;
            if let Some(completion) = run.completion.as_mut() {
                completion.barrier.stopped = true;
            }
        }
        self.check_runtime_completion(svc, context);
    }

    pub(crate) fn check_runtime_completion(&self, svc: &ServiceContext, context: &RuntimeContext) {
        if context.active_turn_id.is_some() || !current_runtime_binding(svc, context) {
            return;
        }
        let messages = runtime_messages(&context.tab_id);
        let summary = {
            let runs = self.runtime_runs.lock();
            let Some(run) = runs
                .get(&context.tab_id)
                .filter(|run| run.nonce == context.binding_nonce && run.ended_turn)
            else {
                return;
            };
            match &run.completion {
                Some(completion) if !completion.barrier.ready(&messages) => return,
                Some(completion) => completion.summary.clone(),
                None if svc.run_completion_for_tab(&context.tab_id)
                    == crate::service::role_policy::RunCompletion::ExplicitFinish =>
                {
                    return
                }
                None => "(agent returned to idle)".into(),
            }
        };
        let _ = self.finish_runtime(
            svc,
            &context.tab_id,
            &context.binding_nonce,
            true,
            false,
            summary,
        );
    }

    pub(crate) fn runtime_process_exited(
        &self,
        svc: &ServiceContext,
        context: &RuntimeContext,
        reason: String,
    ) {
        if current_runtime_binding(svc, context) {
            let _ = self.finish_runtime(
                svc,
                &context.tab_id,
                &context.binding_nonce,
                false,
                false,
                reason,
            );
        }
    }

    fn finish_runtime(
        &self,
        svc: &ServiceContext,
        tab: &str,
        nonce: &str,
        success: bool,
        timed_out: bool,
        summary: String,
    ) -> Option<cron_end::CronEndHandle> {
        self.finish_runtime_owned(svc, tab, RuntimeRunKey { nonce, kind: None, failed_start_epoch: None }, success, timed_out, summary)
    }

    fn finish_runtime_owned(
        &self,
        svc: &ServiceContext,
        tab: &str,
        key: RuntimeRunKey<'_>,
        success: bool,
        timed_out: bool,
        summary: String,
    ) -> Option<cron_end::CronEndHandle> {
        let nonce = key.nonce;
        let cron_kind = self.runtime_runs.lock().get(tab).filter(|run| {
            run.nonce == nonce && key.kind.is_none_or(|kind| kind == &run.kind)
        }).and_then(|run| match &run.kind {
            AgentRunKind::Cron { job_id, run_id } => Some((job_id.clone(), run_id.clone())),
            _ => None,
        });
        if let Some((job_id, run_id)) = cron_kind {
            let (session, cwd) = {
                let mux = svc.mux.read();
                let target = mux.get_tab(tab).filter(|target| target.session_binding_nonce.as_deref() == Some(nonce) && target.pty_id.is_none())?;
                (target.agent_session_id.clone(), target.cwd.clone())
            };
            let output = serde_json::to_string(&runtime_messages(tab)).unwrap_or_default();
            return cron_end::finish(svc, cron_end::EndedCronRun {
                tab_id: tab, job_id: &job_id, run_id: &run_id, success, summary,
                raw_output: &output, session_id: session.as_deref(), cwd: cwd.as_deref(),
                binding_nonce: Some(nonce), pty_id: None,
                failed_start_epoch: key.failed_start_epoch,
            });
        }
        let run = {
            let mut runs = self.runtime_runs.lock();
            if !runs
                .get(tab)
                .is_some_and(|run| run.nonce == nonce && key.kind.is_none_or(|kind| kind == &run.kind) && !(timed_out && run.reported))
            {
                return None;
            }
            let Some(run) = runs.remove(tab) else {
                return None;
            };
            run
        };
        let _ = run.cancelled.send(true);
        let (session, cwd, current) = {
            let mux = svc.mux.read();
            let target = mux.get_tab(tab);
            (
                target.and_then(|target| target.agent_session_id.clone()),
                target.and_then(|target| target.cwd.clone()),
                target.is_none_or(|target| target.session_binding_nonce.as_deref() == Some(nonce)),
            )
        };
        if !current {
            return None;
        }
        let output = serde_json::to_string(&runtime_messages(tab)).unwrap_or_default();
        log::debug!("[run-watcher.runtime] end tab={tab} success={success} timed_out={timed_out} reported={}", run.reported);
        match run.kind {
            AgentRunKind::StepRun { run_id } => crate::service::step_agent_run::finish(
                svc,
                &run_id,
                success,
                summary,
                Ok(runtime_messages(tab)),
            ),
            AgentRunKind::Cron { job_id, run_id } => return cron_end::finish(
                svc,
                cron_end::EndedCronRun {
                    tab_id: tab,
                    job_id: &job_id,
                    run_id: &run_id,
                    success,
                    summary,
                    raw_output: &output,
                    session_id: session.as_deref(),
                    cwd: cwd.as_deref(),
                    binding_nonce: Some(nonce),
                    pty_id: None,
                    failed_start_epoch: None,
                },
            ),
            AgentRunKind::MemWorker { worker_id } => {
                if let Ok(path) = crate::mem::workers::save_worker_log(&worker_id, &output) {
                    svc.workers
                        .set_log_path(&worker_id, path.to_string_lossy().into_owned());
                }
                if let Some(session) = session {
                    svc.workers.set_run_session(&worker_id, session, cwd);
                }
                if !run.reported {
                    let end = if timed_out {
                        codeterm_mem::dream_status::ExecutionEnd::TimedOut {
                            detail: summary.clone(),
                        }
                    } else {
                        codeterm_mem::dream_status::ExecutionEnd::ProcessEnded {
                            detail: if success {
                                format!("execution ended without mem worker report: {summary}")
                            } else {
                                summary.clone()
                            },
                        }
                    };
                    let _ = svc
                        .workers
                        .finish_execution(&worker_id, Some(summary.clone()), end);
                    if timed_out {
                        crate::mem::dream_runs::finalize_dream_at_deadline(
                            svc, &worker_id, &summary,
                        );
                    }
                }
                let _ = svc.close_tab_with_reason(tab, TabCloseReason::AgentRunEnded);
                if let Some(info) = svc.workers.get(&worker_id) {
                    svc.ui.emit(
                        "graph-worker-done",
                        serde_json::to_value(info).unwrap_or_default(),
                    );
                }
            }
        }
        None
    }
}

fn ended_unbound_start(svc: &ServiceContext, tab: &str, nonce: &str) -> Option<u64> {
    let session = engine::for_tab(tab)?;
    let _permit = session.gate.try_acquire().ok()?;
    unbound_start_epoch(svc, &session, tab, nonce)
}

pub(super) fn unbound_start_epoch(svc: &ServiceContext, session: &std::sync::Arc<engine::Session>, tab: &str, nonce: &str) -> Option<u64> {
    let context = session.context.lock().clone();
    let state = session.state.lock().clone();
    let process = session.process.lock().clone();
    (context.binding_nonce == nonce && context.tab_id == tab && context.thread_id.is_none()
        && state.epoch == context.epoch
        && !session.thread_loaded.load(std::sync::atomic::Ordering::Acquire)
        && matches!(state.phase, crate::service::agent_runtime::model::RuntimePhase::Degraded { .. })
        && process.is_none_or(|process| process.ended.borrow().is_some() || process.client.ended().borrow().is_some())
        && engine::for_tab(tab).is_some_and(|current| std::sync::Arc::ptr_eq(&current, session))
        && svc.mux.read().get_tab(tab).is_some_and(|target| target.agent_type.as_deref() == Some(session.provider.as_str()) && target.session_binding_nonce.as_deref() == Some(nonce) && target.agent_session_id.is_none()))
        .then_some(context.epoch)
}

impl RunWatcherRegistry {
    pub(crate) fn bind_cron(&self, owner: CronRunAuthority) -> bool {
        let mut bindings = self.cron_bindings.lock();
        if bindings.get(&owner.tab_id).is_some_and(|prior| prior != &owner) { return false; }
        bindings.insert(owner.tab_id.clone(), owner);
        true
    }

    pub(crate) fn cron_binding(&self, tab: &str, job: &str, run: &str) -> Option<CronRunAuthority> {
        self.cron_bindings.lock().get(tab).filter(|owner| owner.job_id == job && owner.run_id == run).cloned()
    }

    pub(crate) fn retire_cron(&self, owner: &CronRunAuthority) {
        {
            let mut runs = self.runtime_runs.lock();
            let matches = owner.pty_id.is_none() && runs.get(&owner.tab_id).is_some_and(|run| {
                run.nonce.as_str() == owner.binding_nonce.as_deref().unwrap_or_default()
                    && run.kind == (AgentRunKind::Cron { job_id: owner.job_id.clone(), run_id: owner.run_id.clone() })
            });
            if matches {
                if let Some(run) = runs.remove(&owner.tab_id) { let _ = run.cancelled.send(true); }
            }
        }
        self.runs.lock().retain(|_, run| {
            let matches = run.tab_id == owner.tab_id && Some(run.pty_id.as_str()) == owner.pty_id.as_deref()
                && run.binding_nonce == owner.binding_nonce
                && run.kind == (AgentRunKind::Cron { job_id: owner.job_id.clone(), run_id: owner.run_id.clone() });
            if matches { run.completed.store(true, std::sync::atomic::Ordering::Release); }
            !matches
        });
    }

    pub(crate) fn release_cron_binding(&self, owner: &CronRunAuthority) {
        let mut bindings = self.cron_bindings.lock();
        if bindings.get(&owner.tab_id) == Some(owner) { bindings.remove(&owner.tab_id); }
    }

    pub(crate) fn pending_cron_cleanup(&self, owner: &CronRunAuthority) -> Option<std::sync::Arc<crate::pty::manager::PtySession>> {
        self.cron_cleanup.lock().get(&(owner.job_id.clone(), owner.run_id.clone())).filter(|entry| entry.owner == *owner).and_then(|entry| entry.pty.clone())
    }

    pub(crate) fn retain_cron_cleanup(&self, owner: &CronRunAuthority, pty: Option<std::sync::Arc<crate::pty::manager::PtySession>>) {
        let mut pending = self.cron_cleanup.lock();
        let entry = pending.entry((owner.job_id.clone(), owner.run_id.clone())).or_insert_with(|| CronCleanup { owner: owner.clone(), pty: None });
        if entry.owner == *owner && pty.is_some() { entry.pty = pty; }
    }

    pub(crate) fn complete_cron_cleanup(&self, owner: &CronRunAuthority) {
        let mut pending = self.cron_cleanup.lock();
        let key = (owner.job_id.clone(), owner.run_id.clone());
        if pending.get(&key).is_some_and(|entry| entry.owner == *owner) { pending.remove(&key); }
        drop(pending);
        self.release_cron_binding(owner);
    }

    pub(crate) fn cancelled_cron_bindings(&self, store: &crate::cron::store::CronStore, job: &str) -> Vec<CronRunAuthority> {
        let mut owners: Vec<_> = self.cron_bindings.lock().values().filter(|owner| owner.job_id == job && store.run_receipt(job, &owner.run_id).is_some_and(|run| run.status == crate::cron::store::CronRunStatus::Cancelled)).cloned().collect();
        for entry in self.cron_cleanup.lock().values().filter(|entry| entry.owner.job_id == job && store.run_receipt(job, &entry.owner.run_id).is_some() && !store.run_is_active(job, &entry.owner.run_id)) {
            if !owners.contains(&entry.owner) { owners.push(entry.owner.clone()); }
        }
        owners
    }

    pub(crate) fn resume_cron_cleanup_for_run(&self, svc: &ServiceContext, run: &str, caller: Option<&str>, nonce: Option<&str>, human: bool) -> bool {
        let owner = self.cron_cleanup.lock().values().find(|entry| entry.owner.run_id == run && (human || caller == Some(entry.owner.tab_id.as_str()) && nonce == entry.owner.nonce())).map(|entry| entry.owner.clone());
        let Some(owner) = owner else { return false };
        svc.close_owned_cron_launch(&owner, TabCloseReason::AgentRunEnded);
        true
    }

}

async fn runtime_deadline(
    completion: &mut tokio::sync::watch::Receiver<bool>,
    deadline: impl std::future::Future<Output = ()>,
) -> bool {
    if *completion.borrow() {
        return false;
    }
    tokio::select! {
        biased;
        _ = completion.changed() => false,
        _ = deadline => true,
    }
}

fn runtime_messages(tab: &str) -> Vec<ChatMsg> {
    engine::for_tab(tab)
        .map(|session| session.cache.lock().snapshot())
        .unwrap_or_default()
}

fn current_runtime_binding(svc: &ServiceContext, context: &RuntimeContext) -> bool {
    let epoch_matches = engine::for_tab(&context.tab_id).is_some_and(|session| {
        let own = session.context.lock();
        own.epoch == context.epoch
            && own.binding_nonce == context.binding_nonce
            && own.thread_id == context.thread_id
    });
    epoch_matches
        && svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
            tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
                && tab.agent_session_id == context.thread_id
        })
}

impl ServiceContext {
    pub(crate) fn fail_prepared_runtime_cron(
        &self,
        tab: &str,
        metadata: &crate::service::spawn::ChatBackendLaunchMetadata,
        reason: &str,
    ) {
        let Some(crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id }) = &metadata.owner_linkage else {
            return;
        };
        let Some(owner) = self.run_watcher.cron_binding(tab, job_id, run_id) else { return };
        let Some(nonce) = owner.binding_nonce.as_deref() else { return };
        let kind = AgentRunKind::Cron { job_id: job_id.clone(), run_id: run_id.clone() };
        let settlement = if self.run_watcher.has_runtime_run(tab) {
            self.run_watcher.finish_runtime_owned(self, tab, RuntimeRunKey { nonce, kind: Some(&kind), failed_start_epoch: None }, false, false, reason.into())
        } else {
            cron_end::finish(self, cron_end::EndedCronRun {
                tab_id: &owner.tab_id, job_id, run_id, success: false, summary: reason.into(), raw_output: "",
                session_id: None, cwd: metadata.params.cwd.as_deref(), binding_nonce: owner.binding_nonce.as_deref(), pty_id: None, failed_start_epoch: None,
            })
        };
        if let Some(settlement) = settlement {
            if let Err(error) = settlement.wait_for_terminal() {
                log::error!("Cron preparation terminal outcome retained: job={job_id} run={run_id}: {error}");
            }
        }
    }

    pub(crate) fn prepare_runtime_run_watcher(
        &self,
        tab: &str,
        metadata: &crate::service::spawn::ChatBackendLaunchMetadata,
    ) -> Result<(), String> {
        let (kind, timeout) = if let Some(crate::service::spawn::RuntimeOwnerLinkage::StepRun {
            run_id,
            timeout_seconds,
        }) = &metadata.owner_linkage
        {
            (
                AgentRunKind::StepRun {
                    run_id: run_id.clone(),
                },
                *timeout_seconds,
            )
        } else if let Some(crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id }) =
            &metadata.owner_linkage
        {
            let timeout = self
                .try_ext::<crate::CronState>()
                .and_then(|cron| cron.lock().get(job_id).and_then(|job| job.timeout_seconds))
                .unwrap_or(self.configuration().memory.worker_timeout_seconds);
            (
                AgentRunKind::Cron {
                    job_id: job_id.clone(),
                    run_id: run_id.clone(),
                },
                timeout,
            )
        } else if let Some(worker_id) = &metadata.agent.worker_id {
            let intent = self
                .workers
                .get(worker_id)
                .map(|worker| worker.intent)
                .unwrap_or_default();
            (
                AgentRunKind::MemWorker {
                    worker_id: worker_id.clone(),
                },
                crate::mem::worker_deadline::worker_deadline_secs(
                    &intent,
                    &self.configuration().memory,
                ),
            )
        } else {
            return Ok(());
        };
        self.run_watcher.register_runtime(self, tab, kind, timeout)
    }
}

#[cfg(test)]
mod tests;
