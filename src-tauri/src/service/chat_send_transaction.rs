use std::collections::{HashMap, VecDeque};

use parking_lot::Mutex;
use serde::Serialize;

const MAX_RESULTS: usize = 256;
const MAX_IN_FLIGHT: usize = 32;

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
    pub tab_id: String,
    pub state: ChatSendState,
    pub error: Option<String>,
    pub ledger_id: String,
    pub sequence: u64,
    pub retained_from: u64,
}

struct ChatSendLedgerState {
    ledger_id: String,
    next_sequence: u64,
    by_id: HashMap<String, ChatSendStatus>,
    latest_by_tab: HashMap<String, String>,
    latest_failure_by_tab: HashMap<String, String>,
    order: VecDeque<String>,
}

impl Default for ChatSendLedgerState {
    fn default() -> Self {
        Self {
            ledger_id: uuid::Uuid::new_v4().to_string(),
            next_sequence: 1,
            by_id: HashMap::new(),
            latest_by_tab: HashMap::new(),
            latest_failure_by_tab: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct ChatSendLedger {
    state: Mutex<ChatSendLedgerState>,
}

impl ChatSendLedger {
    pub(crate) fn try_start(&self, tab_id: &str) -> Result<ChatSendStatus, String> {
        let mut state = self.state.lock();
        let in_flight = state
            .by_id
            .values()
            .filter(|status| status.state == ChatSendState::Pending)
            .count();
        if in_flight >= MAX_IN_FLIGHT {
            return Err(format!(
                "chat send backpressure: {MAX_IN_FLIGHT} sends are still pending"
            ));
        }
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        let status = ChatSendStatus {
            send_id: uuid::Uuid::new_v4().to_string(),
            tab_id: tab_id.to_string(),
            state: ChatSendState::Pending,
            error: None,
            ledger_id: state.ledger_id.clone(),
            sequence,
            retained_from: sequence,
        };
        state
            .latest_by_tab
            .insert(tab_id.to_string(), status.send_id.clone());
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
            state.latest_by_tab.retain(|_, send_id| send_id != &expired);
            state
                .latest_failure_by_tab
                .retain(|_, send_id| send_id != &expired);
        }
        Ok(Self::with_retention_window(&state, status))
    }

    pub(crate) fn finish(
        &self,
        send_id: &str,
        result: Result<(), String>,
    ) -> Option<ChatSendStatus> {
        let mut state = self.state.lock();
        let failed_tab = {
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
                    Some(status.tab_id.clone())
                }
            }
        };
        if let Some(tab_id) = failed_tab {
            state
                .latest_failure_by_tab
                .insert(tab_id, send_id.to_string());
        }
        let status = state.by_id.get(send_id).cloned()?;
        Some(Self::with_retention_window(&state, status))
    }

    pub(crate) fn reconcile(&self, tab_id: &str) -> Vec<ChatSendStatus> {
        let state = self.state.lock();
        let failures = state
            .order
            .iter()
            .filter_map(|send_id| state.by_id.get(send_id))
            .filter(|status| status.tab_id == tab_id && status.state == ChatSendState::Failed)
            .cloned()
            .map(|status| Self::with_retention_window(&state, status))
            .collect::<Vec<_>>();
        if !failures.is_empty() {
            return failures;
        }
        state
            .latest_by_tab
            .get(tab_id)
            .and_then(|send_id| state.by_id.get(send_id))
            .cloned()
            .map(|status| Self::with_retention_window(&state, status))
            .into_iter()
            .collect()
    }

    fn with_retention_window(
        state: &ChatSendLedgerState,
        mut status: ChatSendStatus,
    ) -> ChatSendStatus {
        status.retained_from = state
            .order
            .front()
            .and_then(|send_id| state.by_id.get(send_id))
            .map_or(state.next_sequence, |retained| retained.sequence);
        status
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
    pub(crate) fn start_chat_send(
        &self,
        tab_id: &str,
        text: String,
    ) -> Result<ChatSendStatus, String> {
        let pending = self.chat_send_ledger.try_start(tab_id)?;
        let response = pending.clone();
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            let status = run_chat_send(&svc.chat_send_ledger, pending, &text, |text| {
                svc.send_text(&tab_id, text, true)
            });
            crate::api::events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
            if let Some(error) = status.error.as_deref() {
                log::warn!(
                    "[chat-send] send_id={} tab={} failed: {}",
                    status.send_id,
                    status.tab_id,
                    error
                );
            }
        });
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_submit_retains_one_payload_and_correlated_result() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1").unwrap();
        let mut writes = Vec::new();
        let result = run_chat_send(&ledger, pending.clone(), "one payload", |text| {
            writes.push(text.to_string());
            Err("submit key was not acknowledged".into())
        });

        assert_eq!(writes, ["one payload"]);
        assert_eq!(result.send_id, pending.send_id);
        assert_eq!(result.state, ChatSendState::Failed);
        assert_eq!(ledger.reconcile("tab-1"), vec![result]);
    }

    #[test]
    fn pending_transaction_survives_ui_detach_and_activation() {
        let ledger = ChatSendLedger::default();
        let pending = ledger.try_start("tab-1").unwrap();

        assert_eq!(ledger.reconcile("tab-1"), vec![pending.clone()]);

        let result = run_chat_send(&ledger, pending, "hello", |_| Ok(()));

        assert_eq!(result.state, ChatSendState::Sent);
        assert_eq!(ledger.reconcile("tab-1"), vec![result]);
    }

    #[test]
    fn a_missed_failure_survives_a_newer_pending_send() {
        let ledger = ChatSendLedger::default();
        let first = ledger.try_start("tab-1").unwrap();
        let second = ledger.try_start("tab-1").unwrap();
        let failed = run_chat_send(&ledger, first, "first", |_| Err("Enter swallowed".into()));

        assert_eq!(ledger.reconcile("tab-1"), vec![failed]);
        assert_eq!(second.state, ChatSendState::Pending);
    }

    #[test]
    fn in_flight_cap_rejects_before_growth_and_releases_on_completion() {
        let ledger = ChatSendLedger::default();
        let mut pending = Vec::new();
        for index in 0..MAX_IN_FLIGHT {
            pending.push(ledger.try_start(&format!("tab-{index}")).unwrap());
        }

        assert!(ledger.try_start("overflow").is_err());
        assert_eq!(ledger.state.lock().by_id.len(), MAX_IN_FLIGHT);

        ledger.finish(&pending[0].send_id, Ok(())).unwrap();
        assert!(ledger.try_start("released").is_ok());
        assert_eq!(ledger.state.lock().by_id.len(), MAX_IN_FLIGHT + 1);
    }

    #[test]
    fn reconcile_retains_every_failure_missed_while_chat_is_detached() {
        let ledger = ChatSendLedger::default();
        let first = ledger.try_start("tab-1").unwrap();
        let second = ledger.try_start("tab-1").unwrap();
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

    #[test]
    fn reconciliation_exposes_the_backend_retention_window() {
        let ledger = ChatSendLedger::default();
        for index in 0..=MAX_RESULTS {
            let pending = ledger.try_start("tab-1").unwrap();
            run_chat_send(&ledger, pending, "message", |_| {
                Err(format!("failure-{index}"))
            });
        }

        let retained = ledger.reconcile("tab-1");
        assert_eq!(retained.len(), MAX_RESULTS);
        assert_eq!(retained[0].sequence, 2);
        assert!(retained.iter().all(|status| status.retained_from == 2));
    }
}
