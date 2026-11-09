use super::ServiceContext;
use codeterm_neighbors::{supervisor::ListenerState, Error};
use serde_json::json;

use codeterm_neighbors::responses::{ListenerFailure, NeighborListenerEvent, NeighborWatchState};

pub(super) fn failure_detail(error: Error, previous: Option<ListenerFailure>) -> ListenerFailure {
    match error {
        Error::ListenProtocol {
            kind: "listen_retries_exhausted",
        } => ListenerFailure {
            kind: "listen_retries_exhausted".into(),
            last_error: Some(previous.map(Box::new)),
            ..Default::default()
        },
        Error::ListenProtocol { kind } => ListenerFailure {
            kind: kind.into(),
            ..Default::default()
        },
        Error::Firestore { code, operation } => ListenerFailure {
            kind: "grpc".into(),
            code: Some(code),
            operation: Some(operation.into()),
            ..Default::default()
        },
        error => ListenerFailure {
            kind: format!("{error:?}"),
            message: Some(error.to_string()),
            ..Default::default()
        },
    }
}

impl ServiceContext {
    pub(super) fn neighbor_watch_failure(&self, watch: &str, error: Error) {
        let version = self.forget_neighbor_friend_receivers();
        self.ui.emit(
            "neighbor-runtime-status",
            json!(NeighborListenerEvent::Unavailable {
                receiver_unavailable: true,
                friendship_id: None,
                friend_receivers_version: Some(version),
                reason: None,
                message: None
            }),
        );
        self.neighbor_watch_state(watch, ListenerState::Reconnecting, Some(error));
        let detail = self
            .neighbor_cell()
            .watch_states
            .lock()
            .get(watch)
            .and_then(|state| state.error.clone());
        self.stop_neighbor_listeners_state(ListenerState::Reconnecting, detail);
    }
    pub(super) fn neighbor_watch_state(
        &self,
        watch: &str,
        state: ListenerState,
        failure: Option<Error>,
    ) {
        let mut watches = self.neighbor_cell().watch_states.lock();
        let previous = watches.get(watch).and_then(|view| view.error.clone());
        let error = if state == ListenerState::Receiving {
            None
        } else {
            failure
                .map(|error| failure_detail(error, previous.clone()))
                .or(previous)
        };
        let view = NeighborWatchState {
            status: state,
            error: error.clone(),
        };
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
                json!(NeighborListenerEvent::Listener {
                    friendship_id: Some(friend),
                    status: state,
                    error: Some(error.clone()),
                    reason: None
                }),
            );
        }
    }
}
