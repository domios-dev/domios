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
            let Some(expired) = state.order.pop_front() else {
                break;
            };
            state.by_id.remove(&expired);
            state
                .latest_by_pane
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
        let status = state.by_id.get_mut(send_id)?;
        match result {
            Ok(()) => {
                status.state = ChatSendState::Sent;
                status.error = None;
            }
            Err(error) => {
                status.state = ChatSendState::Failed;
                status.error = Some(error);
            }
        }
        Some(status.clone())
    }

    pub(crate) fn latest(&self, pane_id: &str) -> Option<ChatSendStatus> {
        let state = self.state.lock();
        let send_id = state.latest_by_pane.get(pane_id)?;
        state.by_id.get(send_id).cloned()
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
}
