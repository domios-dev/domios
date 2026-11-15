use super::super::storage::Settings;
use super::*;
use codeterm_neighbors::{
    adapter::ReceiverStatus,
    receive::InboxEvent,
    supervisor::{InboxSession, ListenerState, ReceptionEvents},
};
use std::collections::BTreeSet;

impl ServiceContext {
    pub(super) fn neighbor_concierge_view(
        &self,
        friendship: &str,
        settings: &Settings,
        receiver: &ReceiverStatus,
    ) -> Value {
        let lapsed = self.neighbor_lapsed_installations();
        json!(self.neighbor_concierge_projection(friendship, settings, receiver, &lapsed))
    }

    /// The concierge follows the `friend_receivers` projection: a receiver whose membership expired is not receiving.
    pub(super) fn neighbor_concierge_projection(
        &self,
        friendship: &str,
        settings: &Settings,
        receiver: &ReceiverStatus,
        lapsed: &BTreeSet<String>,
    ) -> codeterm_neighbors::responses::NeighborConciergeState {
        let gateway = settings
            .bindings
            .get(friendship)
            .filter(|tab| self.validate_neighbor_gateway(tab).is_ok());
        let missing = gateway.is_none();
        let paused = self.neighbor_effective_pause(settings, friendship);
        let enabled = self.configuration().neighbors.enabled;
        let here = enabled && receiver.receiving_here();
        let expired = Self::receiver_membership_expired(receiver, lapsed);
        let state = if !enabled || !receiver.receiving_enabled || expired {
            "not_receiving"
        } else if !here {
            "receiving_elsewhere"
        } else if paused {
            "paused"
        } else {
            self.neighbor_concierge_lifecycle(
                settings.bindings.get(friendship).map(String::as_str),
                settings.permissions.contains_key(friendship),
            )
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
        codeterm_neighbors::responses::NeighborConciergeState {
            concierge_dormant: settings.dormant_concierges.contains_key(friendship),
            reasoning_effort: options.reasoning_effort,
            service_tier: options.service_tier,
            initial_context_digest: initial_context_digest.map(str::to_owned),
            gateway_tab: gateway.cloned(),
            paused,
            my_paused: paused,
            concierge_missing: missing,
            pending_count: count,
            pending_count_capped: count
                .is_some_and(|count| count >= codeterm_neighbors::adapter::PENDING_COUNT_LIMIT),
            receiving_here: here,
            receiver_machine_label: receiver.receiver_machine_label.clone(),
            state: state.into(),
            not_receiving_reason: expired
                .then_some(codeterm_neighbors::responses::NotReceivingReason::MembershipExpired),
        }
    }

    fn neighbor_concierge_lifecycle(
        &self,
        binding: Option<&str>,
        has_permission: bool,
    ) -> &'static str {
        use codeterm_types::pty::BootStage;

        let Some(tab_id) = binding else {
            return "concierge_missing";
        };
        let Some(tab) = self.mux.read().get_tab(tab_id).cloned() else {
            return "concierge_closed";
        };
        if tab.team_id.is_some()
            || tab.role.is_some()
            || crate::service::general_agent::is_general_agent(&tab)
        {
            return "concierge_invalid";
        }
        if self.validate_neighbor_gateway(tab_id).is_err() {
            return "concierge_crashed";
        }
        if !has_permission {
            return "concierge_access_needed";
        }
        let runtime = tab.agent_runtime_state.as_ref();
        let phase = runtime.and_then(|state| state["state"].as_str());
        let code = runtime.and_then(|state| state["failureCode"].as_str());
        if runtime.is_some_and(|state| state["accessFailure"] == "neighbor_restore_grant_required")
        {
            return "concierge_access_needed";
        }
        if phase == Some("loginPending")
            || runtime.is_some_and(|state| state["auth"] == "signedOut")
            || tab.provider_signals.access_lost()
            || tab
                .provider_signals
                .auth
                .as_ref()
                .is_some_and(|auth| !auth.logged_in)
            || code == Some("authentication_required")
        {
            return "concierge_auth_needed";
        }
        if matches!(phase, Some("degraded" | "unloaded")) {
            return if code == Some("provider_refused") {
                "concierge_provider_refused"
            } else if phase == Some("unloaded") && tab.frozen && code.is_none() {
                "concierge_starting"
            } else {
                "concierge_crashed"
            };
        }
        if tab.boot_stage == Some(BootStage::SpawnFailed) {
            return "concierge_provider_refused";
        }
        if tab.agent_type.is_none()
            || tab.exit_suspected_at.is_some()
            || tab.pty_id.as_ref().is_some_and(|pty| {
                self.agent_tracker.lock().get_status(pty) == crate::agent::status::TabStatus::Dead
            })
        {
            return "concierge_crashed";
        }
        if (runtime.is_some() || tab.boot_stage == Some(BootStage::AgentReady))
            && tab.agent_type.as_ref().is_some_and(|provider| {
                self.agent_registry
                    .with_provider(provider, |_| ())
                    .is_none()
            })
        {
            return "concierge_provider_refused";
        }
        if self.neighbor_gateway_ready(tab_id) {
            "ready"
        } else if !tab.frozen && !tab.agent_booting && tab.boot_stage == Some(BootStage::AgentReady)
        {
            "concierge_crashed"
        } else {
            "concierge_starting"
        }
    }

    pub(super) fn neighbor_concierge_views(
        &self,
        settings: &Settings,
        friends: &Value,
    ) -> std::collections::BTreeMap<String, codeterm_neighbors::responses::NeighborConciergeState>
    {
        let mut views = std::collections::BTreeMap::new();
        let lapsed = self.neighbor_lapsed_installations();
        if let Some(friends) = friends["friends"].as_array() {
            for friend in friends {
                if let Some(id) = friend["friendship_id"].as_str() {
                    views.insert(
                        id.into(),
                        self.neighbor_concierge_projection(
                            id,
                            settings,
                            &self.neighbor_friend_receiver(id).unwrap_or_default(),
                            &lapsed,
                        ),
                    );
                }
            }
        }
        views
    }
}

pub(super) async fn watch_lifecycle(session: &listener::Session) {
    let mut events = session.service.events.subscribe();
    let Ok(settings) = session.metadata.settings().await else {
        session.stopped().await;
        return;
    };
    let Some(binding) = settings.bindings.get(&session.friendship) else {
        session.stopped().await;
        return;
    };
    let present = || session.service.mux.read().get_tab(binding).is_some();
    let mut was_present = present();
    let mut previous = Value::Null;
    if publish_projection(session, &settings, &mut previous).is_err() {
        return;
    }
    loop {
        let event = tokio::select! {
            biased;
            _ = session.stopped() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => {
                let codeterm_events::SystemEvent::Generic { event_type, data } = event.event else {
                    continue;
                };
                if event_type == "mux-state-changed" {
                    let is_present = present();
                    if is_present == was_present {
                        continue;
                    }
                    was_present = is_present;
                } else if !matches!(
                    event_type.as_str(),
                    "agent-runtime-state" | "tab-boot-stage" | "tab-status-changed"
                ) || data["tab_id"].as_str() != Some(binding.as_str())
                {
                    continue;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
        }
        if publish_projection(session, &settings, &mut previous).is_err() {
            return;
        }
    }
}

pub(super) async fn publish(session: &listener::Session) -> codeterm_neighbors::Result<()> {
    let settings = session.metadata.settings().await?;
    publish_projection(session, &settings, &mut Value::Null)
}

fn publish_projection(
    session: &listener::Session,
    settings: &Settings,
    previous: &mut Value,
) -> codeterm_neighbors::Result<()> {
    session.current()?;
    let receiver = session
        .service
        .neighbor_friend_receiver(&session.friendship)
        .ok_or(Error::NotReceiver)?;
    let mut view =
        session
            .service
            .neighbor_concierge_view(&session.friendship, settings, &receiver);
    view["friendship_id"] = json!(session.friendship);
    if view != *previous {
        session
            .service
            .ui
            .emit("neighbor-concierge-status", view.clone());
        *previous = view;
    }
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
    loop {
        let event = tokio::select! {
            biased;
            _ = stopped.changed() => return Err(Error::Revoked),
            result = inbox.next() => result?,
        };
        session.current()?;
        match event {
            Some(InboxEvent::Ready) => {
                let count = inbox.pending_count()?;
                session
                    .service
                    .neighbor_cell()
                    .pending_counts
                    .lock()
                    .insert(session.friendship.clone(), count);
                publish(session).await?;
                events.state(ListenerState::Receiving);
            }
            Some(InboxEvent::Messages(_) | InboxEvent::Snapshot(_)) => return Err(Error::Gateway),
            None => return Err(Error::Transport),
        }
    }
}
#[cfg(test)]
mod tests;
