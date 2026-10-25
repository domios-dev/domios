use crate::receive::NeighborTransport;
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
            if let Err(error) = events.retry_after_failure(attempt).await {
                events.state(ListenerState::Stopped);
                return Err(error);
            }
        }
        events.state(ListenerState::Connecting);
        let result = async {
            session.prepare().await?;
            let cursor = session.cursor().await?;
            // Each open authenticates anew; only the durable admitted cursor is resumed.
            let mut listener = transport
                .listen_from_cursor(friendship, cursor.as_ref())
                .await?;
            while let Some(mut messages) = listener.next().await? {
                session.receive(&mut messages).await?;
                events.state(ListenerState::Receiving);
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
