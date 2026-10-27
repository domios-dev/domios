use crate::receive::{InboxEvent, NeighborTransport};
use crate::wire::{Cursor, InboxMessage};
use crate::{Error, Result};
use async_trait::async_trait;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenerState {
    Connecting,
    Receiving,
    Reconnecting,
    Stopped,
}

#[async_trait]
pub trait InboxSession: Send + Sync {
    async fn stopped(&self);
    async fn prepare(&self) -> Result<()>;
    async fn cursor(&self) -> Result<Option<Cursor>>;
    async fn receive(&self, messages: &mut [InboxMessage]) -> Result<()>;
}

#[async_trait]
pub trait ReceptionEvents: Send + Sync {
    fn state(&self, state: ListenerState);
    async fn retry_after_failure(&self, attempt: usize) -> Result<()>;
}

pub async fn supervise(
    transport: &dyn NeighborTransport,
    friendship: &str,
    session: &dyn InboxSession,
    events: &dyn ReceptionEvents,
) -> Result<()> {
    for attempt in 0..=3 {
        if attempt > 0 {
            events.state(ListenerState::Reconnecting);
            let recovery = tokio::select! {
                biased;
                _ = session.stopped() => Err(Error::Revoked),
                result = events.retry_after_failure(attempt) => result,
            };
            if let Err(error) = recovery {
                events.state(ListenerState::Stopped);
                return Err(error);
            }
        }
        events.state(ListenerState::Connecting);
        let result = async {
            session.prepare().await?;
            let cursor = session.cursor().await?;
            let mut listener = tokio::select! {
                biased;
                _ = session.stopped() => return Err(Error::Revoked),
                result = transport.listen_from_cursor(friendship, cursor.as_ref()) => result?,
            };
            loop {
                let event = tokio::select! {
                biased;
                    _ = session.stopped() => return Err(Error::Revoked),
                    result = listener.next() => result?,
                };
                let Some(event) = event else { break };
                match event {
                    InboxEvent::Ready => events.state(ListenerState::Receiving),
                    InboxEvent::Messages(mut messages) => session.receive(&mut messages).await?,
                }
            }
            Err(Error::Transport)
        }
        .await;
        match result {
            Err(Error::Transport | Error::LocalKeyUnavailable | Error::SecretStore)
                if attempt < 3 => {}
            result => {
                events.state(ListenerState::Stopped);
                return result;
            }
        }
    }
    Err(Error::Transport)
}
