use super::{enabled, record};
use crate::agent::chat_types::ChatMsg;
use crate::service::chat_send_transaction::SendEvidenceAnchor;
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn source_id(kind: &str, tab: &str, session: &str, nonce: Option<&str>, turn: &str) -> String {
    serde_json::to_string(&(kind, tab, session, nonce, turn)).unwrap_or_default()
}

pub(crate) fn link_rpc(
    id: &str,
    tab: &str,
    provider: &str,
    session: &str,
    nonce: &str,
    turn: &str,
) {
    if enabled() {
        let source = source_id("rpc", tab, session, Some(nonce), turn);
        record(
            id,
            "trace.link",
            tab,
            provider,
            "protocol_turn",
            Some(&source),
        );
    }
}

#[derive(Clone)]
pub(crate) struct PtyFlow {
    id: String,
    tab: String,
    provider: String,
    confirmed: Arc<AtomicBool>,
}

impl PtyFlow {
    pub(crate) fn transaction(
        svc: &crate::service::ServiceContext,
        pty: &str,
        id: Option<&str>,
        text: &str,
        enter: bool,
    ) -> Option<Self> {
        if !enabled() || !enter || text.is_empty() {
            return None;
        }
        let (tab, provider) = {
            let mux = svc.mux.read();
            let tab = mux.tab_id_for_pty(pty)?;
            let provider = mux.get_tab(&tab)?.agent_type.clone()?;
            (tab, provider)
        };
        let inherited = id
            .map(str::to_string)
            .or_else(|| super::tab_flow(svc, &tab));
        Self::start(inherited.as_deref(), &tab, &provider)
    }

    pub(crate) fn watch(
        &self,
        svc: &crate::service::ServiceContext,
        anchor: Option<&SendEvidenceAnchor>,
        nonce: Option<&str>,
    ) {
        if let Some(anchor) = anchor {
            super::watch_pty(svc, &self.tab, anchor, nonce);
        }
    }

    pub(crate) fn start(id: Option<&str>, tab: &str, provider: &str) -> Option<Self> {
        if !enabled() {
            return None;
        }
        let flow = Self {
            id: id
                .map(str::to_string)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            tab: tab.into(),
            provider: provider.into(),
            confirmed: Arc::new(AtomicBool::new(false)),
        };
        record(
            &flow.id,
            if id.is_some() {
                "send.transport"
            } else {
                "send.started"
            },
            tab,
            provider,
            if id.is_some() {
                "pty"
            } else {
                "fresh_unledgered_pty"
            },
            None,
        );
        Some(flow)
    }

    pub(crate) fn written(&self) {
        record(
            &self.id,
            "send.written",
            &self.tab,
            &self.provider,
            "pty_queue",
            None,
        );
    }

    pub(crate) fn confirmed(
        &self,
        anchor: &SendEvidenceAnchor,
        nonce: Option<&str>,
        user_id: Option<&str>,
    ) {
        if self.confirmed.swap(true, Ordering::Relaxed) {
            return;
        }
        if let Some(user) = user_id.filter(|id| !id.is_empty()) {
            let source = source_id("pty", &self.tab, &anchor.session_id, nonce, user);
            record(
                &self.id,
                "trace.link",
                &self.tab,
                &self.provider,
                "exact_append_window",
                Some(&source),
            );
        }
        record(
            &self.id,
            "send.delivered",
            &self.tab,
            &self.provider,
            "exact_turn",
            None,
        );
        record(
            &self.id,
            "transcript.append",
            &self.tab,
            &self.provider,
            "own_user_turn",
            None,
        );
    }
}

#[derive(Default)]
pub(crate) struct SourceCursor {
    user: Option<String>,
    seen: HashSet<(String, String)>,
}

impl SourceCursor {
    pub(crate) fn pty(
        &mut self,
        tab: &str,
        provider: &str,
        session: &str,
        nonce: Option<&str>,
        messages: &[ChatMsg],
    ) {
        if !enabled() {
            return;
        }
        for message in messages {
            if message.kind == "user" {
                self.user = (!message.id.is_empty())
                    .then(|| source_id("pty", tab, session, nonce, &message.id));
            } else if message.kind == "assistant" {
                if let Some(source) = self.user.clone() {
                    self.assistant(&source, tab, provider, message);
                }
            }
        }
    }

    pub(crate) fn rpc(
        tab: &str,
        provider: &str,
        session: &str,
        nonce: Option<&str>,
        message: &ChatMsg,
    ) {
        if !enabled() || message.kind != "assistant" {
            return;
        }
        if let Some(turn) = message.tool_flow_id.as_deref() {
            let source = source_id("rpc", tab, session, nonce, turn);
            record(
                &source,
                "transcript.assistant",
                tab,
                provider,
                "protocol_turn",
                None,
            );
        }
    }

    fn assistant(&mut self, source: &str, tab: &str, provider: &str, message: &ChatMsg) {
        if self.seen.len() >= 4096 {
            self.seen.clear();
        }
        if self.seen.insert((source.into(), message.id.clone())) {
            record(
                source,
                "transcript.assistant",
                tab,
                provider,
                "source_user_turn",
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests;
