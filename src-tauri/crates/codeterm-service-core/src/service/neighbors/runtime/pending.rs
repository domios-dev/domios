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
        let paused = self.neighbor_effective_pause(settings, friendship);
        let enabled = self.configuration().neighbors.enabled;
        let here = enabled && receiver.receiving_here();
        let state = if !enabled || !receiver.receiving_enabled {
            "not_receiving"
        } else if !here {
            "receiving_elsewhere"
        } else if paused {
            "paused"
        } else if missing {
            "concierge_missing"
        } else if !gateway.is_some_and(|tab| self.neighbor_gateway_ready(tab)) {
            "concierge_starting"
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
        let initial_context_digest = settings.initial_context_digest(friendship);
        let options = self.neighbor_launch_options(settings, friendship);
        json!({"concierge_dormant":settings.dormant_concierges.contains_key(friendship),"reasoning_effort":options.reasoning_effort,"service_tier":options.service_tier,"initial_context_digest":initial_context_digest,"gateway_tab":gateway,"paused":paused,"my_paused":paused,"concierge_missing":missing,"pending_count":count,"pending_count_capped":count.is_some_and(|count| count >= codeterm_neighbors::adapter::PENDING_COUNT_LIMIT),"receiving_here":here,"receiver_machine_label":receiver.receiver_machine_label,"state":state})
    }

    pub(super) fn neighbor_concierge_views(
        &self,
        settings: &Settings,
        account_receiver: &ReceiverStatus,
        friends: &Value,
    ) -> Value {
        let mut views = serde_json::Map::new();
        if let Some(friends) = friends["friends"].as_array() {
            for friend in friends {
                if let Some(id) = friend["friendship_id"].as_str() {
                    views.insert(
                        id.into(),
                        self.neighbor_concierge_view(
                            id,
                            settings,
                            &self
                                .neighbor_friend_receiver(id)
                                .unwrap_or_else(|| ReceiverStatus {
                                    is_me: false,
                                    receiving_enabled: false,
                                    ..account_receiver.clone()
                                }),
                        ),
                    );
                }
            }
        }
        Value::Object(views)
    }
}

pub(super) async fn publish(session: &listener::Session) -> codeterm_neighbors::Result<()> {
    let settings = session.metadata.settings().await?;
    session.current()?;
    let receiver = session
        .service
        .neighbor_friend_receiver(&session.friendship)
        .ok_or(Error::NotReceiver)?;
    let mut view =
        session
            .service
            .neighbor_concierge_view(&session.friendship, &settings, &receiver);
    view["friendship_id"] = json!(session.friendship);
    session.service.ui.emit("neighbor-concierge-status", view);
    Ok(())
}

struct PendingWatch<'a> {
    session: &'a listener::Session,
    progress: AtomicU64,
    health: parking_lot::Mutex<codeterm_neighbors::supervisor::StreamHealth>,
}
#[async_trait::async_trait]
impl ReceptionEvents for PendingWatch<'_> {
    fn failure(&self, error: Error) {
        self.session.failure(error);
    }
    fn state(&self, state: ListenerState) {
        use codeterm_neighbors::supervisor::RetryClock;
        if state == ListenerState::Receiving
            && self
                .health
                .lock()
                .observe(codeterm_neighbors::supervisor::RuntimeClock.now(), true)
        {
            self.progress.fetch_add(1, Ordering::AcqRel);
        }
        self.session.state(state);
    }
    async fn retry_after_failure(&self, attempt: usize) -> codeterm_neighbors::Result<()> {
        self.session.retry_after_failure(attempt).await
    }
}
#[async_trait::async_trait]
impl codeterm_neighbors::supervisor::DocumentSession for PendingWatch<'_> {
    fn progress(&self) -> u64 {
        self.progress.load(Ordering::Acquire)
    }
    async fn stopped(&self) {
        self.session.stopped().await;
    }
    async fn listen(&self) -> codeterm_neighbors::Result<()> {
        *self.health.lock() = Default::default();
        let result = observe(self.session, self).await;
        self.session
            .service
            .neighbor_cell()
            .pending_counts
            .lock()
            .remove(&self.session.friendship);
        let _ = publish(self.session).await;
        result
    }
}
pub(super) async fn run(session: &listener::Session) {
    let watch = PendingWatch {
        session,
        progress: AtomicU64::new(0),
        health: Default::default(),
    };
    let _ = codeterm_neighbors::supervisor::supervise_documents(&watch).await;
}

#[derive(Default)]
struct Recount {
    in_flight: bool,
    dirty: bool,
}

impl Recount {
    fn changed(&mut self) -> bool {
        if self.in_flight {
            self.dirty = true;
            false
        } else {
            self.in_flight = true;
            true
        }
    }
    fn completed(&mut self) -> bool {
        self.in_flight = std::mem::take(&mut self.dirty);
        self.in_flight
    }
}

type CountFuture<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = codeterm_neighbors::Result<u64>> + Send + 'a>,
>;

async fn observe(
    session: &listener::Session,
    events: &dyn ReceptionEvents,
) -> codeterm_neighbors::Result<()> {
    tokio::select! {
        result = observe_count(session,events) => result,
        result = super::files::prefetch(session) => result,
    }
}

async fn observe_count(
    session: &listener::Session,
    events: &dyn ReceptionEvents,
) -> codeterm_neighbors::Result<()> {
    session.prepare().await?;
    let mut stopped = session.stopped.clone();
    let mut inbox = tokio::select! {
        biased;
        _ = session.stopped() => return Err(Error::Revoked),
        result = session.transport.listen_pending(&session.friendship) => result?,
    };
    let mut recount = Recount::default();
    let mut count: Option<CountFuture<'_>> = None;
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => return Err(Error::Revoked),
            result = async {
                match count.as_mut() { Some(future) => future.await, None => std::future::pending().await }
            } => {
                session.current()?;
                count = None;
                session
                    .service
                    .neighbor_cell()
                    .pending_counts
                    .lock()
                    .insert(session.friendship.clone(), result?.min(codeterm_neighbors::adapter::PENDING_COUNT_LIMIT));
                publish(session).await?;
                events.state(ListenerState::Receiving);
                if recount.completed() { count = Some(Box::pin(session.transport.pending_count(&session.friendship))); }
            }
            result = inbox.next() => {
                session.current()?;
                match result? {
                    Some(InboxEvent::Ready) => if recount.changed() { count = Some(Box::pin(session.transport.pending_count(&session.friendship))); },
                    Some(InboxEvent::Messages(_) | InboxEvent::Snapshot(_)) => return Err(Error::Gateway),
                    None => return Err(Error::Transport),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
