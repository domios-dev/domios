use super::{ServiceContext, TabReopenGuards};
use crate::multiplexer::types::TabBacking;
use crate::service::agent_runtime::{engine, model::RuntimeFailure};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ChatBackendCloseEvidence {
    Witnessed,
    RestoredRuntimeAbsent,
}

pub(crate) struct ChatBackendCloseReceipt {
    pub(crate) tab_id: String,
    pub(crate) session_id: String,
    pub(crate) binding_nonce: Option<String>,
    pub(crate) evidence: ChatBackendCloseEvidence,
}

impl ServiceContext {
    pub(crate) fn chatbackend_close_confirmed(
        &self,
        tab_id: &str,
    ) -> Result<ChatBackendCloseReceipt, String> {
        let (plugin_id, session_id, nonce) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("chat backend tab is absent")?;
            let TabBacking::ChatBackend {
                plugin_id,
                session_id,
            } = &tab.backing
            else {
                return Err("tab has no chat backend".into());
            };
            (
                plugin_id.clone(),
                session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        };
        let guards = self.ext::<Arc<TabReopenGuards>>();
        let deadline = Instant::now() + Duration::from_secs(5);
        guards.serialized_until(tab_id, deadline, || {
            let runtime = engine::for_tab(tab_id);
            let process = runtime
                .as_ref()
                .and_then(|session| session.process.lock().clone());
            let key = (plugin_id.clone(), session_id.clone());
            let bound = guards.sessions.lock().get(&key).cloned();
            if let Some(runtime) = &runtime {
                let context = runtime.context.lock();
                if runtime.provider != plugin_id
                    || context.tab_id != tab_id
                    || Some(context.binding_nonce.as_str()) != nonce.as_deref()
                    || context.thread_id.as_deref() != Some(session_id.as_str())
                {
                    return Err("chat backend runtime identity changed before close".into());
                }
            }
            let evidence = if runtime.is_none() && bound.is_none() {
                let mux = self.mux.read();
                let tab = mux.get_tab(tab_id).ok_or("chat backend tab is absent")?;
                if !tab.restored_from_snapshot
                    || tab.session_binding_nonce != nonce
                    || !matches!(&tab.backing, TabBacking::ChatBackend {
                        plugin_id: current_plugin, session_id: current_session,
                    } if current_plugin == &plugin_id && current_session == &session_id)
                    || guards.sessions.lock().values().any(|session| session.tab_id == tab_id)
                {
                    return Err(format!(
                        "chat backend absence is unproven; inspect with `domios tab inspect --tab {tab_id} --lifecycle` and recover with `domios agent restart --tab {tab_id}`"
                    ));
                }
                ChatBackendCloseEvidence::RestoredRuntimeAbsent
            } else {
                ChatBackendCloseEvidence::Witnessed
            };
            if let Some(bound) = &bound {
                if bound.tab_id != tab_id {
                    return Err("chat backend adapter belongs to another tab".into());
                }
                bound.fail("chat backend session is closing");
                bound
                    .provider
                    .close_session_confirmed(&session_id, deadline)
                    .map_err(|error| error.to_string())?;
            }
            if let Some(runtime) = runtime {
                engine::stop_confirmed_blocking(self, tab_id, runtime)?;
            }
            if process.as_ref().is_some_and(|process| {
                !matches!(
                    *process.ended.borrow(),
                    Some(RuntimeFailure::ProcessExited { .. })
                )
            }) {
                return Err("chat backend process exit was not confirmed".into());
            }
            if let Some(bound) = bound {
                let mut sessions = guards.sessions.lock();
                if !sessions
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(current, &bound))
                {
                    return Err("chat backend adapter changed during close".into());
                }
                sessions.remove(&key);
            }
            {
                let mut starts = guards.turn_starts.lock();
                if starts.get(tab_id).is_some_and(|entry| Some(entry.nonce.as_str()) == nonce.as_deref()) {
                    starts.remove(tab_id);
                }
            }
            crate::service::tab_chat::chatbackend_history::ChatBackendHistory::release(
                &plugin_id,
                &session_id,
            );
            Ok(ChatBackendCloseReceipt {
                tab_id: tab_id.into(),
                session_id,
                binding_nonce: nonce,
                evidence,
            })
        })
    }
}

#[cfg(test)]
mod tests;
