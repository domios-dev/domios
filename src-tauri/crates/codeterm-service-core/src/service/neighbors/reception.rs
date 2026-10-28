use super::ServiceContext;
use codeterm_neighbors::{supervisor::ListenerState, Error};
use serde_json::{json, Value};

pub(super) fn failure_detail(error: Error, previous: Option<Value>) -> Value {
    match error {
        Error::ListenProtocol {
            kind: "listen_retries_exhausted",
        } => json!({"kind":"listen_retries_exhausted","last_error":previous}),
        Error::ListenProtocol { kind } => json!({"kind":kind}),
        Error::Firestore { code, operation } => {
            json!({"kind":"grpc","code":code,"operation":operation})
        }
        error => json!({"kind":format!("{error:?}"),"message":error.to_string()}),
    }
}

impl ServiceContext {
    pub(super) fn neighbor_watch_failure(&self, watch: &str, error: Error) {
        self.neighbor_watch_state(watch, ListenerState::Reconnecting, Some(error));
        let detail = self
            .neighbor_cell()
            .watch_states
            .lock()
            .get(watch)
            .and_then(|state| state.get("error"))
            .cloned();
        self.stop_neighbor_listeners_state(ListenerState::Reconnecting, detail);
    }
    pub(super) fn neighbor_watch_state(
        &self,
        watch: &str,
        state: ListenerState,
        failure: Option<Error>,
    ) {
        let mut watches = self.neighbor_cell().watch_states.lock();
        let previous = watches
            .get(watch)
            .and_then(|view| view.get("error"))
            .filter(|value| !value.is_null())
            .cloned();
        let error = if state == ListenerState::Receiving {
            None
        } else {
            failure
                .map(|error| failure_detail(error, previous.clone()))
                .or(previous)
        };
        let view = json!({"status":state,"error":error});
        watches.insert(watch.into(), view.clone());
        drop(watches);
        self.ui.emit(
            "neighbors-watch-status",
            json!({"watch":watch,"status":state,"error":error}),
        );
        if state == ListenerState::Receiving {
            return;
        }
        let friends: Vec<_> = self.neighbor_cell().states.lock().keys().cloned().collect();
        for friend in friends {
            self.neighbor_cell()
                .states
                .lock()
                .insert(friend.clone(), state);
            if let Some(error) = &error {
                self.neighbor_cell()
                    .failures
                    .lock()
                    .insert(friend.clone(), error.clone());
            }
            self.ui.emit(
                "neighbor-runtime-status",
                json!({"friendship_id":friend,"status":state,"error":error}),
            );
        }
    }
}
