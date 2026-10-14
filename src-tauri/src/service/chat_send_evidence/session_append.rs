use std::sync::Arc;

use crate::agent::registry::AgentRegistry;
use crate::service::tab_chat::{ChatRegistry, DeltaSink, TailConfig};
use crate::service::ServiceContext;

use super::{CapturedSendEvidence, EvidenceWindow};

const EVIDENCE_SUBSCRIBER: &str = "session-send-evidence";

pub(crate) fn ensure_append_stream(
    cfg: TailConfig,
    nonce: Option<&str>,
    registry: Arc<AgentRegistry>,
    chat_registry: &ChatRegistry,
    sink: Arc<dyn DeltaSink>,
) -> bool {
    if chat_registry.acquire(&cfg.tab_id, &cfg.session_id, nonce, EVIDENCE_SUBSCRIBER) {
        return true;
    }
    let handle = crate::service::tab_chat::tailer::spawn_sourced_tailer(
        TailConfig {
            tab_id: cfg.tab_id.clone(),
            provider_id: cfg.provider_id,
            cwd: cfg.cwd,
            session_id: cfg.session_id.clone(),
            start_offset: cfg.start_offset,
        },
        Some(chat_registry.jsonl_source(&cfg.tab_id)),
        registry,
        sink,
    );
    chat_registry.insert(
        cfg.tab_id,
        cfg.session_id,
        nonce.map(str::to_string),
        EVIDENCE_SUBSCRIBER.into(),
        handle,
    )
}

impl ServiceContext {
    pub(crate) fn ensure_chat_send_append_stream(
        &self,
        tab_id: &str,
        evidence: &CapturedSendEvidence,
    ) {
        let Some(binding) = evidence.live_binding.as_ref() else {
            return;
        };
        let (Some(session), Some(nonce)) = (
            binding.session_id.as_deref(),
            binding.binding_nonce.as_deref(),
        ) else {
            return;
        };
        let start_offset = match evidence.anchor.as_ref().map(|anchor| &anchor.window) {
            Some(EvidenceWindow::Jsonl { transcript_offset }) => *transcript_offset,
            None if evidence.absence_witness.is_some() => 0,
            _ => return,
        };
        let cfg = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if !tab.launch_session_authenticated()
                || tab.agent_session_id.as_deref() != Some(session)
                || tab.session_binding_nonce.as_deref() != Some(nonce)
            {
                return;
            }
            let Some(provider_id) = tab.agent_type.clone() else {
                return;
            };
            TailConfig {
                tab_id: tab_id.into(),
                provider_id,
                cwd: tab.cwd.clone().unwrap_or_default(),
                session_id: session.into(),
                start_offset,
            }
        };
        let sink = crate::service::tab_chat::structured_sink::make_flatten_sink(
            self.lifecycle_journal.clone(),
            self.app_handle.clone(),
            self.mux.clone(),
            0,
            Some(session.into()),
            Some(nonce.into()),
            Some(self.chat_send_ledger.clone()),
        );
        log::debug!(
            "[session-turn-evidence] ensure-append-stream tab={} session={} start_offset={}",
            tab_id,
            session,
            start_offset,
        );
        ensure_append_stream(
            cfg,
            Some(nonce),
            self.agent_registry.clone(),
            &self.chat_registry,
            sink,
        );
    }
}
