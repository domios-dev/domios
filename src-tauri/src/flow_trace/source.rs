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
        text: &str,
        enter: bool,
    ) {
        if let Some(anchor) = anchor {
            super::watch_pty(svc, &self.tab, anchor, nonce);
        } else if enter {
            self.watch_unledgered(svc, text);
        }
    }

    pub(crate) fn watch_unledgered(&self, svc: &crate::service::ServiceContext, text: &str) {
        let bound = {
            let mux = svc.mux.read();
            mux.get_tab(&self.tab).and_then(|tab| {
                let nonce = tab.session_binding_nonce.as_ref()?;
                if tab.reported_session_binding_nonce.as_ref() != Some(nonce) {
                    return None;
                }
                Some((
                    tab.agent_type.clone()?,
                    tab.cwd.clone().unwrap_or_default(),
                    tab.agent_session_id.clone()?,
                    nonce.clone(),
                ))
            })
        };
        let Some((provider, cwd, session, nonce)) = bound else {
            self.observer("binding_unavailable");
            return;
        };
        let captured = svc.agent_registry.with_provider(&provider, |provider| {
            (
                provider.session_jsonl_path(&cwd, &session),
                provider.turn_evidence_normalization(),
            )
        });
        let Some((Some(path), normalization)) = captured else {
            self.observer("path_unavailable");
            return;
        };
        let source = svc.chat_registry.jsonl_source(&self.tab);
        let Some(window) = super::watch::capture_window(&source, &session, &path) else {
            self.observer("source_unreadable");
            return;
        };
        let anchor = SendEvidenceAnchor {
            provider_id: provider,
            cwd,
            session_id: session,
            window,
            turn_normalization: normalization,
        };
        super::watch::ensure_unledgered(
            svc,
            &self.tab,
            &anchor,
            Some(&nonce),
            self.clone(),
            text,
            path,
        );
    }

    pub(super) fn observer(&self, outcome: &str) {
        record(
            &self.id,
            "trace.observer",
            &self.tab,
            &self.provider,
            outcome,
            None,
        );
    }

    pub(super) fn id(&self) -> &str {
        &self.id
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

pub(crate) struct UnledgeredObservation {
    flow: PtyFlow,
    payload: String,
    user: Option<String>,
    source_seen: bool,
}

impl UnledgeredObservation {
    pub(super) fn observer(&self, outcome: &str) {
        self.flow.observer(outcome);
    }

    pub(crate) fn new(flow: PtyFlow, payload: String, anchor: &SendEvidenceAnchor) -> Self {
        Self {
            flow,
            payload: crate::service::chat_send_transaction::normalize_turn_evidence_with(
                &payload,
                anchor.turn_normalization,
            ),
            user: None,
            source_seen: false,
        }
    }

    pub(crate) fn observe(
        &mut self,
        anchor: &SendEvidenceAnchor,
        nonce: Option<&str>,
        messages: &[ChatMsg],
        source: &SourceCursor,
    ) -> bool {
        if !self.source_seen && !messages.is_empty() {
            self.observer("first_append");
            self.source_seen = true;
        }
        if self.user.is_none() && !self.payload.is_empty() {
            self.user = messages
                .iter()
                .find(|message| {
                    message.kind == "user"
                        && !message.id.is_empty()
                        && crate::service::chat_send_transaction::normalize_turn_evidence_with(
                            &message.content,
                            anchor.turn_normalization,
                        ) == self.payload
                })
                .map(|message| message.id.clone());
            if let Some(user) = self.user.as_deref() {
                self.flow.confirmed(anchor, nonce, Some(user));
                self.payload.clear();
            }
        }
        self.user
            .as_deref()
            .is_some_and(|user| source.answered(&self.flow.tab, &anchor.session_id, nonce, user))
    }
}

#[derive(Default)]
pub(crate) struct SourceCursor {
    user: Option<String>,
    seen: HashSet<(String, String)>,
}

impl SourceCursor {
    pub(super) fn answered(
        &self,
        tab: &str,
        session: &str,
        nonce: Option<&str>,
        user: &str,
    ) -> bool {
        let source = source_id("pty", tab, session, nonce, user);
        self.seen.iter().any(|(id, _)| id == &source)
    }

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
