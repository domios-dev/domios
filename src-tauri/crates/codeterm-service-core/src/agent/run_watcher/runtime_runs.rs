use super::{cron_end, AgentRunKind, RunWatcherRegistry};
use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::TabCloseReason;
use crate::service::agent_runtime::engine;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::RuntimeContext;
use std::time::Duration;
use tauri::Manager;

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

impl RunWatcherRegistry {
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
        let nonce = {
            let mux = svc.mux.read();
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
                    kind,
                    armed: false,
                    ended_turn: false,
                    completion: None,
                    reported: false,
                    cancelled,
                },
            );
        }
        {
            let owner = svc.clone();
            let tab = tab.to_string();
            tauri::async_runtime::spawn(async move {
                if tokio::time::timeout(Duration::from_secs(timeout_secs), completion.changed())
                    .await
                    .is_err()
                {
                    let watcher = owner.run_watcher.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        watcher.finish_runtime(
                            &owner,
                            &tab,
                            &nonce,
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

    pub(super) fn request_runtime_cron_completion(
        &self,
        svc: &ServiceContext,
        tab: &str,
        run_id: &str,
        summary: &str,
    ) -> Option<Result<(), String>> {
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
                None => "(agent returned to idle)".into(),
            }
        };
        self.finish_runtime(
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
            self.finish_runtime(
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
    ) {
        let run = {
            let mut runs = self.runtime_runs.lock();
            if !runs
                .get(tab)
                .is_some_and(|run| run.nonce == nonce && !(timed_out && run.reported))
            {
                return;
            }
            let Some(run) = runs.remove(tab) else {
                return;
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
            return;
        }
        let output = serde_json::to_string(&runtime_messages(tab)).unwrap_or_default();
        log::debug!("[run-watcher.runtime] end tab={tab} success={success} timed_out={timed_out} reported={}", run.reported);
        match run.kind {
            AgentRunKind::Cron { job_id, run_id } => cron_end::finish(
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
                    codeterm_events::emit_everywhere(&svc.app_handle, "graph-worker-done", info);
                }
            }
        }
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
    pub(crate) fn prepare_runtime_run_watcher(
        &self,
        tab: &str,
        metadata: &crate::service::spawn::ChatBackendLaunchMetadata,
    ) -> Result<(), String> {
        let (kind, timeout) =
            if let Some(crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id }) =
                &metadata.owner_linkage
            {
                let timeout = self
                    .app_handle
                    .try_state::<crate::CronState>()
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
