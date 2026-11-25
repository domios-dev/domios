use super::*;

pub(super) struct RuntimeMemWorkerEnd<'a> {
    pub(super) tab: &'a str,
    pub(super) worker_id: &'a str,
    pub(super) reported: bool,
    pub(super) success: bool,
    pub(super) timed_out: bool,
    pub(super) summary: String,
    pub(super) output: &'a str,
    pub(super) session: Option<String>,
    pub(super) cwd: Option<String>,
}

impl RunWatcherRegistry {
    /// Retire any active runtime or PTY watcher owned by `worker_id` so a
    /// cancelled worker's deadline timer can never fire later and overwrite
    /// its log or diary.
    pub fn retire_worker(&self, worker_id: &str) -> bool {
        let mut retired = false;
        {
            let mut runs = self.runtime_runs.lock();
            let matching_tabs: Vec<String> = runs
                .iter()
                .filter_map(|(tab, run)| match &run.kind {
                    AgentRunKind::MemWorker { worker_id: id } if id == worker_id => {
                        Some(tab.clone())
                    }
                    _ => None,
                })
                .collect();
            for tab in matching_tabs {
                if let Some(run) = runs.remove(&tab) {
                    let _ = run.cancelled.send(true);
                    retired = true;
                }
            }
        }
        self.runs.lock().retain(|_, run| {
            let matches = matches!(
                &run.kind,
                AgentRunKind::MemWorker { worker_id: id } if id == worker_id
            );
            if matches {
                run.completed
                    .store(true, std::sync::atomic::Ordering::Release);
                retired = true;
            }
            !matches
        });
        retired
    }
}

pub(super) fn finish_runtime_mem_worker(svc: &ServiceContext, end: RuntimeMemWorkerEnd<'_>) {
    // A worker already terminated outside this watcher (e.g. cancelled) must not
    // have its log overwritten with empty runtime messages or its diary rewritten
    // as a deadline cut-short.
    if !end.reported
        && svc
            .workers
            .get(end.worker_id)
            .is_some_and(|w| w.status.is_terminal() && w.execution_end.is_some())
    {
        return;
    }
    if let Ok(path) = crate::mem::workers::save_worker_log(end.worker_id, end.output) {
        svc.workers
            .set_log_path(end.worker_id, path.to_string_lossy().into_owned());
    }
    if let Some(session) = end.session {
        svc.workers.set_run_session(end.worker_id, session, end.cwd);
    }
    if !end.reported {
        let execution_end = if end.timed_out {
            codeterm_mem::dream_status::ExecutionEnd::TimedOut {
                detail: end.summary.clone(),
            }
        } else {
            codeterm_mem::dream_status::ExecutionEnd::ProcessEnded {
                detail: if end.success {
                    format!("execution ended without mem worker report: {}", end.summary)
                } else {
                    end.summary.clone()
                },
            }
        };
        let _ =
            svc.workers
                .finish_execution(end.worker_id, Some(end.summary.clone()), execution_end);
        if end.timed_out {
            crate::mem::dream_runs::finalize_dream_at_deadline(svc, end.worker_id, &end.summary);
        }
    }
    let _ = svc.close_tab_with_reason(end.tab, TabCloseReason::AgentRunEnded);
    if let Some(info) = svc.workers.get(end.worker_id) {
        svc.ui.emit(
            "graph-worker-done",
            serde_json::to_value(info).unwrap_or_default(),
        );
    }
}
