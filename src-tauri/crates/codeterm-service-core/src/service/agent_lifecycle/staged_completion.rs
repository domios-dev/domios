use std::time::Instant;

use crate::service::ServiceContext;

impl ServiceContext {
    pub fn staged_model_turn_completed(&self, tab_id: &str, session_id: &str, nonce: &str) {
        let accepted = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.agent_session_id.as_deref() == Some(session_id)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
        });
        log::info!("[agent-model/completion] tab={tab_id} phase=staged_model_turn_completed accepted={accepted} at_ms={}", crate::service::pty_frame_timing::unix_ms());
        if accepted {
            let epoch = self
                .ext::<super::RestartTracker>()
                .lock()
                .staged_models
                .start_epoch(tab_id);
            let reading = Instant::now();
            let anchor = completion_anchor(self, tab_id, session_id);
            log::info!(
                "[agent-model/completion] tab={tab_id} phase=completion_anchor_read elapsed_ms={}",
                reading.elapsed().as_millis()
            );
            let mux = self.mux.read();
            if mux.get_tab(tab_id).is_some_and(|tab| {
                tab.agent_session_id.as_deref() == Some(session_id)
                    && tab.session_binding_nonce.as_deref() == Some(nonce)
                    && tab.reported_session_binding_nonce.as_deref() == Some(nonce)
            }) {
                let mut tracker = self.ext::<super::RestartTracker>().lock();
                if tracker.staged_models.start_epoch(tab_id) != epoch {
                    return;
                }
                tracker
                    .staged_models
                    .signal_completed(tab_id, session_id, nonce);
                if let Some(anchor) = anchor.filter(|anchor| anchor.session_id == session_id) {
                    tracker
                        .staged_models
                        .latch_completed(tab_id, nonce, epoch, anchor);
                }
            }
        }
    }
}

fn completion_anchor(
    svc: &ServiceContext,
    tab_id: &str,
    session_id: &str,
) -> Option<crate::service::chat_send_transaction::SendEvidenceAnchor> {
    let (provider_id, cwd) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id)?;
        if tab.agent_session_id.as_deref() != Some(session_id) {
            return None;
        }
        (tab.agent_type.clone()?, tab.cwd.clone()?)
    };
    let (path, turn_normalization) =
        svc.agent_registry.with_provider(&provider_id, |provider| {
            (
                provider.session_jsonl_path(&cwd, session_id),
                provider.turn_evidence_normalization(),
            )
        })?;
    let offset = std::fs::metadata(path?).ok()?.len();
    Some(crate::service::chat_send_transaction::SendEvidenceAnchor {
        provider_id,
        cwd,
        session_id: session_id.into(),
        turn_normalization,
        window: crate::service::chat_send_transaction::EvidenceWindow::Jsonl {
            transcript_offset: offset,
        },
    })
}
