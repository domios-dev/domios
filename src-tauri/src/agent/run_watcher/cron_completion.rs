use std::collections::HashSet;

use super::{finish_run, AgentRunKind, Ordering, RunWatcherRegistry};
use crate::agent::chat_types::ChatMsg;
use crate::service::ServiceContext;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Default)]
struct CompletionBarrier {
    stopped: bool,
    assistant_ids: HashSet<String>,
}

impl CompletionBarrier {
    fn ready(&self, messages: &[ChatMsg]) -> bool {
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

pub(super) struct PendingCompletion {
    barrier: CompletionBarrier,
    summary: String,
    provider: String,
    cwd: String,
    session: String,
    nonce: String,
    _watcher: Option<RecommendedWatcher>,
    #[cfg(test)]
    events: std::sync::Arc<parking_lot::Mutex<Vec<String>>>,
    #[cfg(test)]
    source_path: Option<std::path::PathBuf>,
}

impl RunWatcherRegistry {
    pub(crate) fn request_cron_completion(
        &self,
        svc: &ServiceContext,
        tab_id: &str,
        run_id: &str,
        summary: &str,
    ) -> Result<(), String> {
        let state = self.runs.lock().values().find(|state| state.tab_id == tab_id
            && matches!(&state.kind, AgentRunKind::Cron { run_id: own, .. } if own == run_id))
            .cloned().ok_or("cron run has no active watcher")?;
        if state.cron_completion.lock().is_some() {
            return Ok(());
        }
        let (provider, cwd, session, nonce) = {
            let mux = svc.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("cron tab closed")?;
            (
                tab.agent_type.clone().ok_or("cron provider missing")?,
                tab.cwd.clone().unwrap_or_default(),
                tab.agent_session_id.clone().ok_or("cron session missing")?,
                tab.session_binding_nonce
                    .clone()
                    .ok_or("cron binding missing")?,
            )
        };
        let messages = if crate::service::codex_rpc::lifecycle::for_tab(tab_id).is_some() {
            crate::service::codex_rpc::transcript::snapshot(svc, tab_id)?.0
        } else {
            crate::service::tab_chat::session_chat_snapshot(svc, &provider, &cwd, &session, 0)?.0
        };
        let path = svc
            .agent_registry
            .with_provider(&provider, |p| p.session_file_path(&cwd, &session))
            .flatten();
        #[cfg(test)]
        let source_path = path.clone();
        let mut watcher = None;
        #[cfg(test)]
        let events = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        if let Some(path) = path {
            let observed = path.clone();
            let service = svc.clone();
            let tab = tab_id.to_string();
            #[cfg(test)]
            let recorded_events = events.clone();
            let mut source_watcher =
                notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                    log::debug!(
                        "[cron] transcript event tab={tab} path={observed:?} event={event:?}"
                    );
                    #[cfg(test)]
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
        *state.cron_completion.lock() = Some(PendingCompletion {
            barrier: CompletionBarrier {
                stopped: false,
                assistant_ids: messages.iter().map(|message| message.id.clone()).collect(),
            },
            summary: summary.into(),
            provider,
            cwd,
            session,
            nonce,
            _watcher: watcher,
            #[cfg(test)]
            events,
            #[cfg(test)]
            source_path,
        });
        log::info!("[cron] completion requested; awaiting completed turn and durable answer: tab={tab_id} run={run_id}");
        Ok(())
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
        tauri::async_runtime::spawn(async move {
            let messages = if let Some(sidecar) =
                crate::service::codex_rpc::lifecycle::for_tab(&state.tab_id)
            {
                match crate::service::codex_rpc::transcript::history(&sidecar.client, &scope.2)
                    .await
                {
                    Ok(history)
                        if history.active_turn.is_none() && !history.completed_turns.is_empty() =>
                    {
                        history
                            .messages
                            .into_iter()
                            .filter(|message| {
                                message.kind != "assistant"
                                    || history.final_assistant_ids.contains(&message.id)
                            })
                            .collect()
                    }
                    _ => return,
                }
            } else {
                let reader = service.clone();
                let (provider, cwd, session) = (scope.0.clone(), scope.1.clone(), scope.2.clone());
                match tauri::async_runtime::spawn_blocking(move || {
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
                    tab.agent_session_id.as_deref() == Some(&scope.2)
                        && tab.session_binding_nonce.as_deref() == Some(&scope.3)
                });
            if !current {
                log::debug!(
                    "[cron] pending completion binding changed tab={}",
                    state.tab_id
                );
                return;
            }
            let summary = {
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
                pending.summary.clone()
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
            let _ = tauri::async_runtime::spawn_blocking(move || {
                finish_run(&finisher, &ending, true, summary)
            })
            .await;
            service.run_watcher.runs.lock().remove(&state.pty_id);
        });
    }
}

#[cfg(test)]
mod tests;
