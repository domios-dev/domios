use super::super::storage::Settings;
use super::*;
use codeterm_neighbors::{
    adapter::ReceiverStatus,
    receive::InboxEvent,
    supervisor::{InboxSession, ListenerState, ReceptionEvents},
};

impl ServiceContext {
    pub(super) fn neighbor_concierge_view(
        &self,
        friendship: &str,
        settings: &Settings,
        receiver: &ReceiverStatus,
    ) -> Value {
        let gateway = settings
            .bindings
            .get(friendship)
            .filter(|tab| self.validate_neighbor_gateway(tab).is_ok());
        let missing = gateway.is_none();
        let enabled = self.configuration().neighbors.enabled;
        let here = enabled && receiver.receiving_here();
        let state = if !enabled || !receiver.receiving_enabled {
            "not_receiving"
        } else if !here {
            "receiving_elsewhere"
        } else if missing {
            "concierge_missing"
        } else {
            "ready"
        };
        let count = if here && missing {
            self.neighbor_cell()
                .pending_counts
                .lock()
                .get(friendship)
                .copied()
        } else {
            None
        };
        json!({"gateway_tab":gateway,"concierge_missing":missing,"pending_count":count,"receiving_here":here,"receiver_machine_label":receiver.receiver_machine_label,"state":state})
    }

    pub(super) fn neighbor_concierge_views(
        &self,
        settings: &Settings,
        receiver: &ReceiverStatus,
        friends: &Value,
    ) -> Value {
        let mut views = serde_json::Map::new();
        if let Some(friends) = friends["friends"].as_array() {
            for friend in friends {
                if let Some(id) = friend["friendship_id"].as_str() {
                    views.insert(
                        id.into(),
                        self.neighbor_concierge_view(id, settings, receiver),
                    );
                }
            }
        }
        Value::Object(views)
    }
}

async fn publish(session: &listener::Session) -> codeterm_neighbors::Result<()> {
    let settings = session.metadata.settings().await?;
    session.current()?;
    let receiver = session
        .service
        .neighbor_cell()
        .receiver
        .lock()
        .clone()
        .ok_or(Error::NotReceiver)?;
    let mut view =
        session
            .service
            .neighbor_concierge_view(&session.friendship, &settings, &receiver);
    view["friendship_id"] = json!(session.friendship);
    session.service.ui.emit("neighbor-concierge-status", view);
    Ok(())
}

pub(super) async fn run(session: &listener::Session) {
    for attempt in 0..=3 {
        if attempt > 0 {
            session.state(ListenerState::Reconnecting);
            let retry = tokio::select! {
                biased;
                _ = session.stopped() => Err(Error::Revoked),
                result = session.retry_after_failure(attempt) => result,
            };
            if retry.is_err() {
                return;
            }
        }
        session.state(ListenerState::Connecting);
        let result = observe(session).await;
        session
            .service
            .neighbor_cell()
            .pending_counts
            .lock()
            .remove(&session.friendship);
        let _ = publish(session).await;
        if let Err(error) = result {
            session.failure(error);
        }
        match result {
            Err(
                Error::Transport
                | Error::Firestore { .. }
                | Error::ListenProtocol { .. }
                | Error::SecretStore
                | Error::LocalKeyUnavailable,
            ) if attempt < 3 => {}
            _ => {
                session.state(ListenerState::Stopped);
                return;
            }
        }
    }
}

async fn observe(session: &listener::Session) -> codeterm_neighbors::Result<()> {
    session.prepare().await?;
    let mut stopped = session.stopped.clone();
    let mut inbox = tokio::select! {
        biased;
        _ = session.stopped() => return Err(Error::Revoked),
        result = session.transport.listen_pending(&session.friendship) => result?,
    };
    loop {
        let event = tokio::select! {
            biased;
            _ = stopped.changed() => return Err(Error::Revoked),
            result = inbox.next() => result?,
        };
        session.current()?;
        match event {
            Some(InboxEvent::Ready) => {
                let count = tokio::select! {
                    biased;
                    _ = stopped.changed() => return Err(Error::Revoked),
                    result = session.transport.pending_count(&session.friendship) => result?,
                };
                session.current()?;
                session
                    .service
                    .neighbor_cell()
                    .pending_counts
                    .lock()
                    .insert(session.friendship.clone(), count);
                publish(session).await?;
                session.state(ListenerState::Receiving);
            }
            Some(InboxEvent::Messages(_)) => return Err(Error::Gateway),
            None => return Err(Error::Transport),
        }
    }
}

#[cfg(test)]
mod tests;
