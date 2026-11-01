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
    fn failure(&self, _error: Error) {}
    fn state(&self, state: ListenerState);
    async fn retry_after_failure(&self, attempt: usize) -> Result<()>;
}

pub async fn supervise(
    transport: &dyn NeighborTransport,
    friendship: &str,
    session: &dyn InboxSession,
    events: &dyn ReceptionEvents,
) -> Result<()> {
    supervise_with(transport, friendship, session, events, &RuntimeClock).await
}

pub async fn supervise_with(
    transport: &dyn NeighborTransport,
    friendship: &str,
    session: &dyn InboxSession,
    events: &dyn ReceptionEvents,
    clock: &dyn RetryClock,
) -> Result<()> {
    let mut attempt: usize = 0;
    loop {
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
        if attempt == 0 {
            events.state(ListenerState::Connecting);
        }
        let result = async {
            session.prepare().await?;
            let cursor = session.cursor().await?;
            let mut listener = tokio::select! {
                biased;
                _ = session.stopped() => return Err(Error::Revoked),
                result = transport.listen_from_cursor(friendship, cursor.as_ref()) => result?,
            };
            let mut health = StreamHealth::default();
            let mut ready = false;
            loop {
                let event = tokio::select! {
                biased;
                    _ = session.stopped() => return Err(Error::Revoked),
                    result = listener.next() => result?,
                };
                let Some(event) = event else { break };
                match event {
                    InboxEvent::Ready => {
                        health = StreamHealth::default();
                        health.observe(clock.now(), false);
                        ready = true;
                        events.state(ListenerState::Receiving);
                    }
                    InboxEvent::Snapshot(mut messages) => session.receive(&mut messages).await?,
                    InboxEvent::Messages(mut messages) => {
                        let incremental = !messages.is_empty();
                        session.receive(&mut messages).await?;
                        if ready && health.observe(clock.now(), incremental) {
                            attempt = 0;
                        }
                    }
                }
            }
            Err(Error::Transport)
        }
        .await;
        if let Err(error) = result {
            events.failure(error);
        }
        match result {
            Err(error) if retryable(error) => {
                attempt = attempt.saturating_add(1);
            }
            result => {
                events.state(ListenerState::Stopped);
                return result;
            }
        }
    }
}

/// A document watch re-reads its authoritative snapshot on each connection.
#[async_trait]
pub trait DocumentSession: ReceptionEvents {
    fn progress(&self) -> u64 {
        0
    }
    async fn stopped(&self);
    async fn listen(&self) -> Result<()>;
}

pub async fn supervise_documents(session: &dyn DocumentSession) -> Result<()> {
    let mut attempt: usize = 0;
    loop {
        if attempt > 0 {
            session.state(ListenerState::Reconnecting);
            tokio::select! {
                biased;
                _ = session.stopped() => return Err(Error::Revoked),
                result = session.retry_after_failure(attempt) => result?,
            }
        }
        if attempt == 0 {
            session.state(ListenerState::Connecting);
        }
        let progress = session.progress();
        let outcome = tokio::select! {
            biased;
            _ = session.stopped() => return Err(Error::Revoked),
            result = session.listen() => result,
        };
        let error = outcome.err().unwrap_or(Error::Transport);
        session.failure(error);
        if session.progress() != progress {
            attempt = 0;
        }
        if !retryable(error) {
            session.state(ListenerState::Stopped);
            return Err(error);
        }
        attempt = attempt.saturating_add(1);
    }
}

pub fn retryable(error: Error) -> bool {
    matches!(
        error,
        Error::Transport
            | Error::Http {
                status: 401 | 429 | 500..=599,
                ..
            }
            | Error::RateLimited { .. }
            | Error::AggregateLimited { .. }
            | Error::CursorStore
            | Error::LocalKeyUnavailable
            | Error::SecretStore
            | Error::Firestore {
                code: 1 | 2 | 4 | 8 | 10 | 13 | 14 | 16,
                ..
            }
            | Error::ListenProtocol {
                kind: "existence_filter_retry_limit"
                    | "existence_filter_mismatch"
                    | "stream_closed"
                    | "request_stream_closed"
                    | "target_removed"
            }
    )
}

pub fn reconnect_delay(attempt: usize, jitter: u64) -> std::time::Duration {
    let floor = (500u64 << attempt.saturating_sub(1).min(10)).min(300_000);
    let ceiling = floor.saturating_mul(2).min(300_000);
    std::time::Duration::from_millis(floor + jitter.saturating_sub(floor) % (ceiling - floor + 1))
}

#[derive(Default)]
pub struct StreamHealth {
    snapshot_at: Option<std::time::Duration>,
}

impl StreamHealth {
    pub fn observe(&mut self, now: std::time::Duration, incremental: bool) -> bool {
        match self.snapshot_at {
            None => {
                self.snapshot_at = Some(now);
                false
            }
            Some(at) => incremental && now.saturating_sub(at) >= std::time::Duration::from_secs(30),
        }
    }
}

#[async_trait]
pub trait RetryClock: Send + Sync {
    fn now(&self) -> std::time::Duration {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed()
    }
    fn jitter(&self) -> u64;
    async fn wait(&self, delay: std::time::Duration);
}

pub struct RuntimeClock;

#[async_trait]
impl RetryClock for RuntimeClock {
    fn jitter(&self) -> u64 {
        rand::random()
    }
    async fn wait(&self, delay: std::time::Duration) {
        tokio::time::sleep_until(tokio::time::Instant::now() + delay).await;
    }
}

pub async fn reconnect_backoff_with(clock: &dyn RetryClock, attempt: usize) {
    clock.wait(reconnect_delay(attempt, clock.jitter())).await;
}

pub async fn reconnect_backoff(attempt: usize) {
    reconnect_backoff_with(&RuntimeClock, attempt).await;
}

#[cfg(test)]
mod tests;
