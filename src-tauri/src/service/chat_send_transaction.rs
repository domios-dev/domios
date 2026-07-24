use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use serde::Serialize;

const MAX_RESULTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatSendState {
    Pending,
    Sent,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChatSendStatus {
    pub send_id: String,
    pub pane_id: String,
    pub state: ChatSendState,
    pub error: Option<String>,
}

#[derive(Default)]
struct ChatSendLedgerState {
    by_id: HashMap<String, ChatSendStatus>,
    latest_by_pane: HashMap<String, String>,
    latest_failure_by_pane: HashMap<String, String>,
    order: VecDeque<String>,
}

#[derive(Default)]
pub(crate) struct ChatSendLedger {
    state: Mutex<ChatSendLedgerState>,
}

impl ChatSendLedger {
    pub(crate) fn start(&self, pane_id: &str) -> ChatSendStatus {
        let status = ChatSendStatus {
            send_id: uuid::Uuid::new_v4().to_string(),
            pane_id: pane_id.to_string(),
            state: ChatSendState::Pending,
            error: None,
        };
        let mut state = self.state.lock();
        state
            .latest_by_pane
            .insert(pane_id.to_string(), status.send_id.clone());
        state.order.push_back(status.send_id.clone());
        state.by_id.insert(status.send_id.clone(), status.clone());
        while state.order.len() > MAX_RESULTS {
            let Some(position) = state.order.iter().position(|send_id| {
                state
                    .by_id
                    .get(send_id)
                    .is_some_and(|status| status.state != ChatSendState::Pending)
            }) else {
                break;
            };
            let expired = state
                .order
                .remove(position)
                .expect("the selected terminal result exists");
            state.by_id.remove(&expired);
            state
                .latest_by_pane
                .retain(|_, send_id| send_id != &expired);
            state
                .latest_failure_by_pane
                .retain(|_, send_id| send_id != &expired);
        }
        status
    }

    pub(crate) fn finish(
        &self,
        send_id: &str,
        result: Result<(), String>,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let failed_pane = {
            let status = state.by_id.get_mut(send_id)?;
            match result {
                Ok(()) => {
                    status.state = ChatSendState::Sent;
                    status.error = None;
                    None
                }
                Err(error) => {
                    status.state = ChatSendState::Failed;
                    status.error = Some(error);
                    Some(status.pane_id.clone())
                }
            }
        };
        if let Some(pane_id) = failed_pane {
            state
                .latest_failure_by_pane
                .insert(pane_id, send_id.to_string());
        }
        state.by_id.get(send_id).cloned()
    }

    pub(crate) fn latest(&self, pane_id: &str) -> Option<ChatSendStatus> {
        let state = self.state.lock();
        let send_id = state
            .latest_failure_by_pane
            .get(pane_id)
            .or_else(|| state.latest_by_pane.get(pane_id))?;
        state.by_id.get(send_id).cloned()
    }

    pub(crate) fn reconcile(&self, pane_id: &str) -> Vec<ChatSendStatus> {
        let state = self.state.lock();
        let failures = state
            .order
            .iter()
            .filter_map(|send_id| state.by_id.get(send_id))
            .filter(|status| status.pane_id == pane_id && status.state == ChatSendState::Failed)
            .cloned()
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return failures;
        }
        state
            .latest_by_pane
            .get(pane_id)
            .and_then(|send_id| state.by_id.get(send_id))
            .cloned()
            .into_iter()
            .collect()
    }
}

pub(crate) fn run_chat_send(
    ledger: &ChatSendLedger,
    pending: ChatSendStatus,
    text: &str,
    submit: impl FnOnce(&str) -> Result<(), String>,
) -> ChatSendStatus {
    let result = submit(text);
    ledger
        .finish(&pending.send_id, result)
        .expect("a started Chat send remains in the bounded ledger until completion")
}

impl crate::service::ServiceContext {
    pub(crate) fn start_chat_send(&self, pane_id: &str, text: String) -> ChatSendStatus {
        let pending = self.chat_send_ledger.start(pane_id);
        let response = pending.clone();
        let svc = self.clone();
        let pane_id = pane_id.to_string();
        std::thread::spawn(move || {
            let status = run_chat_send(&svc.chat_send_ledger, pending, &text, |text| {
                svc.send_text(&pane_id, text, true)
            });
            crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
            if let Some(error) = status.error.as_deref() {
                log::warn!(
                    "[chat-send] send_id={} pane={} failed: {}",
                    status.send_id,
                    status.pane_id,
                    error
                );
            }
        });
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_submit_retains_one_payload_and_correlated_result() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.start("tab-1");
        let mut writes = Vec::new();
        let result = run_chat_send(&ledger, pending.clone(), "one payload", |text| {
            writes.push(text.to_string());
            Err("submit key was not acknowledged".into())
        });

        assert_eq!(writes, ["one payload"]);
        assert_eq!(result.send_id, pending.send_id);
        assert_eq!(result.state, ChatSendState::Failed);
        assert_eq!(ledger.latest("tab-1"), Some(result));
    }

    #[test]
    fn pending_transaction_survives_ui_detach_and_activation() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.start("tab-1");

        assert_eq!(ledger.latest("tab-1"), Some(pending.clone()));

        let result = run_chat_send(&ledger, pending, "hello", |_| Ok(()));

        assert_eq!(result.state, ChatSendState::Sent);
        assert_eq!(ledger.latest("tab-1"), Some(result));
    }

    #[test]
    fn a_missed_failure_survives_a_newer_pending_send() {
        let ledger = ChatSendLedger::default();
        let first = ledger.start("tab-1");
        let second = ledger.start("tab-1");
        let failed = run_chat_send(&ledger, first, "first", |_| Err("Enter swallowed".into()));

        assert_eq!(ledger.latest("tab-1"), Some(failed));
        assert_eq!(second.state, ChatSendState::Pending);
    }

    #[test]
    fn capacity_never_evicts_an_in_flight_transaction() {
        let ledger = ChatSendLedger::default();
        let first = ledger.start("tab-0");
        for index in 1..=MAX_RESULTS {
            ledger.start(&format!("tab-{index}"));
        }

        assert_eq!(ledger.latest("tab-0"), Some(first));
    }

    #[test]
    fn reconcile_retains_every_failure_missed_while_chat_is_detached() {
        let ledger = ChatSendLedger::default();
        let first = ledger.start("tab-1");
        let second = ledger.start("tab-1");
        run_chat_send(&ledger, first, "first", |_| Err("first failed".into()));
        run_chat_send(&ledger, second, "second", |_| Err("second failed".into()));

        assert_eq!(
            ledger
                .reconcile("tab-1")
                .into_iter()
                .filter_map(|status| status.error)
                .collect::<Vec<_>>(),
            ["first failed", "second failed"]
        );
    }
}
