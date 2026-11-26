use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::Duration,
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    store::{read_private, write_private, MAX_RECORD_BYTES},
    AppendReceipt, ConciergeEpoch, TranscriptContent, TranscriptInput, TranscriptRange,
    TranscriptStore,
};
use crate::service::spawn_chatbackend::live_transcript::{
    transcript_router_stuck, transcript_router_turn_ms,
};
use crate::service::{
    agent_runtime::{engine, restart},
    dictation::live_capture::{
        LiveCaptureEvent, LiveCaptureIdentity, LiveCaptureOutput, LiveCaptureSink,
    },
    wall_now_ms, ServiceContext,
};

mod delivery;
mod forward;
mod local;
mod scout;
mod training_log;
pub(in crate::service) use delivery::{external_handoff, is_transcript_delivery_token};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptTrigger {
    #[default]
    ScoutThenConcierge,
    WakeWord,
    DirectBatch,
    Manual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoutStatus {
    Ready,
    Unavailable,
    Disabled,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTranscriptStatus {
    pub committed_seq: u64,
    pub binding: super::TranscriptBinding,
    pub trigger: TranscriptTrigger,
    pub scout: ScoutStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scout_reason: Option<String>,
    pub awaiting_delivery: bool,
    pub concierge: ConciergeStatus,
    pub configured: bool,
    pub default_mic_tap: bool,
    pub router: Option<TranscriptRouterInfo>,
    pub reviewing_through_seq: Option<u64>,
    pub last_route_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRouterInfo {
    pub provider: String,
    pub model: Option<String>,
    pub cap: usize,
    pub activity: RouterActivity,
    pub active_turn_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RouterActivity {
    Idle,
    Reviewing,
    Stuck,
    Unavailable,
}

impl TranscriptRouterInfo {
    fn new(provider: String, model: Option<String>) -> Self {
        Self {
            provider,
            model,
            cap: super::DEFAULT_TRANSCRIPT_TURN_LIMIT.get(),
            activity: RouterActivity::Idle,
            active_turn_ms: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConciergeStatus {
    Ready,
    NeedsAttachment,
    Unavailable,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct RoutingState {
    trigger: TranscriptTrigger,
    attempted_through: u64,
    pending: Option<delivery::PendingAction>,
}

#[derive(Default)]
struct Dispatch {
    owners: Mutex<HashMap<String, OwnerDispatch>>,
}

#[derive(Default)]
struct OwnerDispatch {
    running: bool,
    waiting_for_idle: bool,
    pending: Option<TranscriptRange>,
    last_route_error: Option<String>,
    dispatch_finished: Arc<tokio::sync::Notify>,
}

struct CaptureSink(Weak<ServiceContext>);

impl ServiceContext {
    pub fn install_live_transcript_capture(self: &Arc<Self>) {
        let _ = self.register_host_extension(Arc::new(LiveCaptureOutput(Arc::new(CaptureSink(
            Arc::downgrade(self),
        )))));
    }

    pub fn live_transcript_status(&self, owner: &str) -> Result<LiveTranscriptStatus, String> {
        let store = self.open_live_transcript(owner)?;
        let mut entries = self.ext_or_init(Dispatch::default).owners.lock();
        let mut state = store.routing_state()?;
        let running = entries.get(owner).is_some_and(|entry| entry.running);
        let awaiting_delivery = state.pending.is_some();
        if !running && awaiting_delivery {
            if let Err(error) = delivery::settle(self, &store, &mut state) {
                entries
                    .entry(owner.into())
                    .or_default()
                    .settle_route(Err(error));
            }
        }
        let entry = entries.get(owner);
        let trigger = state.trigger;
        let last_route_error = entry.and_then(|entry| entry.last_route_error.clone());
        drop(entries);
        if awaiting_delivery && state.pending.is_none() {
            self.emit_live_transcript_changed(owner);
        }
        let binding = store.binding();
        let local_endpoint = if binding.target == super::BindingTarget::Local {
            local::endpoint(self)
        } else {
            None
        };
        let scout_decision =
            scout::resolve(self, binding.target == super::BindingTarget::Local, trigger).decision;
        let scout = scout_decision.status();
        let scout_reason = scout_decision.reason();
        let epoch = match &binding.target {
            super::BindingTarget::Attached { epoch_id } => store
                .epoch(epoch_id)
                .map_err(|error| error.to_string())?
                .map(|entry| entry.epoch),
            _ => None,
        };
        let attaching = running && matches!(&binding.target, super::BindingTarget::Detached);
        let router = if matches!(binding.target, super::BindingTarget::Local) {
            Some(local::router_info(self, local_endpoint.as_ref(), running))
        } else if let Some(epoch) = &epoch {
            let mut info = TranscriptRouterInfo::new(epoch.provider.clone(), epoch.model.clone());
            info.active_turn_ms = transcript_router_turn_ms(epoch);
            info.activity = router_activity(
                self.ready_router_tab(owner, epoch),
                info.active_turn_ms,
                running || state.pending.is_some(),
            );
            Some(info)
        } else if attaching {
            store
                .suspended_epoch()
                .ok()
                .flatten()
                .map(|previous| {
                    TranscriptRouterInfo::new(previous.epoch.provider, previous.epoch.model)
                })
                .or_else(|| self.transcript_router_preview())
                .map(|mut info| {
                    info.activity = RouterActivity::Reviewing;
                    info
                })
        } else {
            self.transcript_router_preview()
        };
        let reviewing_through_seq = router
            .as_ref()
            .filter(|info| info.activity == RouterActivity::Reviewing)
            .map(|_| state.attempted_through);
        let concierge = match &binding.target {
            super::BindingTarget::Local if local_endpoint.is_some() => ConciergeStatus::Ready,
            super::BindingTarget::Attached { .. }
                if epoch.is_some()
                    && router
                        .as_ref()
                        .is_some_and(|info| info.activity != RouterActivity::Unavailable) =>
            {
                ConciergeStatus::Ready
            }
            super::BindingTarget::External { tab_id }
                if self
                    .mux
                    .read()
                    .get_tab(tab_id)
                    .is_some_and(engine::has_live_bound_session) =>
            {
                ConciergeStatus::Ready
            }
            super::BindingTarget::Detached if attaching && router.is_some() => {
                ConciergeStatus::Ready
            }
            super::BindingTarget::Detached if router.is_some() => ConciergeStatus::NeedsAttachment,
            _ => ConciergeStatus::Unavailable,
        };
        let setup = self.configuration().live_transcript;
        Ok(LiveTranscriptStatus {
            committed_seq: store.committed_seq(),
            binding,
            trigger,
            scout,
            scout_reason,
            awaiting_delivery: state.pending.is_some(),
            concierge,
            configured: setup.configured,
            default_mic_tap: setup.default_mic_tap,
            router,
            reviewing_through_seq,
            last_route_error,
        })
    }

    pub fn schedule_live_transcript_retention(self: &Arc<Self>, store: TranscriptStore) {
        super::maintenance::schedule(self, store);
    }

    pub(in crate::service) fn transcript_router_cwd(&self, owner: &str) -> String {
        Self::normalize_resolved_agent_cwd(
            &self
                .agent_pad_data_dir
                .join("transcripts")
                .join(owner)
                .join("concierge")
                .to_string_lossy(),
        )
    }

    pub(in crate::service) fn transcript_router_matches_setup(
        &self,
        owner: &str,
        epoch: &super::ConciergeEpoch,
    ) -> bool {
        let Ok((provider, model)) = self.transcript_router_selection(Some(epoch)) else {
            return false;
        };
        let cwd = self.transcript_router_cwd(owner);
        provider == epoch.provider
            && self
                .mux
                .read()
                .get_tab(&epoch.runtime_tab_id)
                .is_some_and(|tab| {
                    let launch_model = tab
                        .agent_runtime_config
                        .as_ref()
                        .and_then(|config| config["model"].as_str());
                    tab.cwd.as_deref() == Some(cwd.as_str())
                        && (model == epoch.model
                            || model
                                .as_deref()
                                .is_some_and(|model| Some(model) == launch_model))
                })
    }

    pub fn configure_live_transcript(
        &self,
        owner: &str,
        configure: bool,
        provider: Option<String>,
        model: Option<String>,
        trigger: Option<TranscriptTrigger>,
        default_mic_tap: Option<bool>,
    ) -> Result<(), String> {
        let configure = configure || provider.is_some() || model.is_some();
        if !configure && default_mic_tap.is_none() {
            return Ok(());
        }
        self.open_live_transcript(owner)?;
        let trigger = if configure {
            trigger
                .map(serde_json::to_value)
                .transpose()
                .map_err(|error| error.to_string())?
        } else {
            None
        };
        let next = crate::config::update_config_and_apply(
            self.ext::<crate::config::ConfigState>(),
            |config| {
                let setup = &mut config.live_transcript;
                setup.configured |= configure;
                if let Some(provider) = provider {
                    if setup.provider != provider {
                        setup.model.clear();
                    }
                    setup.provider = provider;
                }
                if let Some(model) = model {
                    setup.model = model;
                }
                if let Some(trigger) = trigger.as_ref().and_then(serde_json::Value::as_str) {
                    setup.trigger = trigger.into();
                }
                if let Some(default_mic_tap) = default_mic_tap {
                    setup.default_mic_tap = default_mic_tap;
                }
                Ok(())
            },
        )?;
        self.ui.emit(
            "config-changed",
            serde_json::to_value(next).map_err(|error| error.to_string())?,
        );
        self.emit_live_transcript_changed(owner);
        Ok(())
    }

    fn emit_live_transcript_changed(&self, owner: &str) {
        self.events
            .publish_generic("live-transcript-changed", json!({"ownerTabId": owner}));
    }

    pub fn with_live_transcript_consumer<T>(
        &self,
        owner: &str,
        caller: &crate::service::tab_credentials::TabCaller,
        revision: u64,
        use_binding: impl FnOnce(&TranscriptStore, &super::TranscriptBinding) -> T,
    ) -> Result<T, String> {
        let owners = self.ext_or_init(super::TranscriptOwners::default);
        owners.ensure_loaded(&self.agent_pad_data_dir)?;
        let refused = || "Transcript access requires the exact current consumer".to_string();
        let store = owners.lookup_live(owner).ok_or_else(refused)?;
        let binding = store.binding();
        if binding.revision != revision {
            return Err(refused());
        }
        let mux = self.mux.read();
        let tab = mux.get_tab(&caller.tab_id).ok_or_else(refused)?;
        if tab.session_binding_nonce.as_deref() != Some(caller.binding_nonce.as_str()) {
            return Err(refused());
        }
        let authorized = match &binding.target {
            super::BindingTarget::External { tab_id } => {
                tab_id == &caller.tab_id && !tab.is_transcript_router()
            }
            super::BindingTarget::Attached { epoch_id } => {
                store.epoch(epoch_id).ok().flatten().is_some_and(|history| {
                    let epoch = history.epoch;
                    history.archive.is_none()
                        && epoch.runtime_tab_id == caller.tab_id
                        && epoch.binding_nonce == caller.binding_nonce
                        && Some(epoch.session_id.as_str()) == tab.agent_session_id.as_deref()
                        && tab.transcript_router_owner() == Some(owner)
                })
            }
            super::BindingTarget::Local | super::BindingTarget::Detached => false,
        };
        if !authorized || mux.get_tab(owner).is_none() {
            return Err(refused());
        }
        store
            .verify_live_binding(owner, &binding)
            .map_err(|_| refused())?;
        drop(mux);
        Ok(use_binding(&store, &binding))
    }

    pub(crate) fn wake_live_transcript_on_router_idle(&self, router_tab_id: &str) {
        let Some(svc) = self.weak_self().upgrade() else {
            return;
        };
        let Some(owners) = self.try_ext::<super::TranscriptOwners>() else {
            return;
        };
        for (owner, store) in owners.stores_for_destination(router_tab_id) {
            self.emit_live_transcript_changed(&owner);
            let committed = store.committed_seq();
            if committed > 0
                && store.routing_state().is_ok_and(|state| {
                    state.trigger != TranscriptTrigger::Manual
                        && committed > state.attempted_through
                })
            {
                enqueue(&svc, &store, &owner, committed);
            }
        }
    }

    pub fn set_transcript_external_override(
        &self,
        owner: &str,
        external: Option<&str>,
        start: super::CursorStart,
    ) -> Result<super::TranscriptBinding, String> {
        let target = external.map_or(super::BindingTarget::Detached, |id| {
            super::BindingTarget::External { tab_id: id.into() }
        });
        self.replace_transcript_binding(owner, target, start)
    }

    pub fn update_live_transcript_binding(
        &self,
        owner: &str,
        external: Option<&str>,
        start: super::CursorStart,
        trigger: Option<TranscriptTrigger>,
    ) -> Result<super::TranscriptBinding, String> {
        let store = self.open_live_transcript(owner)?;
        let mut entries = self.ext_or_init(Dispatch::default).owners.lock();
        let entry = entries.entry(owner.into()).or_default();
        if entry.running {
            return Err("A transcript dispatch is in progress".into());
        }
        let mut state = store.routing_state()?;
        delivery::settle(self, &store, &mut state)?;
        if state.pending.is_some() {
            return Err("A transcript delivery is awaiting confirmation".into());
        }
        let trigger = trigger.or_else(|| {
            let setup = self.configuration().live_transcript;
            (setup.configured && !store.inner.root.join("routing.json").exists()).then(|| {
                let scout = scout::resolve(self, false, TranscriptTrigger::ScoutThenConcierge)
                    .decision
                    .status();
                default_trigger(&setup.trigger, scout)
            })
        });
        entry.running = true;
        drop(entries);
        let result = (|| {
            let previous_target = store.binding().target;
            let binding = match external {
                Some(tab) => self.set_transcript_external_override(owner, Some(tab), start)?,
                None if local::selected(self, &store) => local::bind(self, owner)?,
                None => {
                    if matches!(
                        store.binding().target,
                        super::BindingTarget::External { .. }
                    ) {
                        self.set_transcript_external_override(owner, None, start)?;
                    }
                    self.admit_transcript_router(owner, super::DEFAULT_TRANSCRIPT_TURN_LIMIT, true)?
                        .binding()
                        .clone()
                }
            };
            let entering_local = binding.target == super::BindingTarget::Local
                && previous_target != super::BindingTarget::Local;
            if entering_local {
                state.attempted_through = store
                    .cursor(&binding)
                    .map_err(|error| error.to_string())?
                    .acked_seq;
            }
            if let Some(trigger) = trigger {
                state.trigger = trigger;
            }
            if entering_local || trigger.is_some() {
                store.save_routing_state(&state)?;
            }
            Ok(binding)
        })();
        if let Some(entry) = self
            .ext_or_init(Dispatch::default)
            .owners
            .lock()
            .get_mut(owner)
        {
            entry.running = false;
            entry.pending = None;
            entry.dispatch_finished.notify_waiters();
            if result.is_ok() {
                entry.last_route_error = None;
            }
        }
        self.emit_live_transcript_changed(owner);
        result
    }

    pub fn route_live_transcript_manually(
        &self,
        owner: &str,
        range: TranscriptRange,
    ) -> Result<(), String> {
        let store = self.open_live_transcript(owner)?;
        let dispatch = self.ext_or_init(Dispatch::default);
        let mut entries = dispatch.owners.lock();
        let entry = entries.entry(owner.into()).or_default();
        if entry.running {
            return Err("A transcript dispatch is in progress".into());
        }
        let mut state = store.routing_state()?;
        delivery::settle(self, &store, &mut state)?;
        if state.trigger != TranscriptTrigger::Manual || state.pending.is_some() {
            return Err("Manual routing is unavailable".into());
        }
        if range.from_seq == 0
            || range.from_seq > range.to_seq
            || range.to_seq > store.committed_seq()
            || range.from_seq <= state.attempted_through
        {
            return Err("Select a new committed transcript range".into());
        }
        entry.running = true;
        drop(entries);
        self.emit_live_transcript_changed(owner);
        let binding = store.binding();
        let result = if matches!(binding.target, super::BindingTarget::Local)
            || matches!(binding.target, super::BindingTarget::Detached)
                && local::selected(self, &store)
        {
            local::route_range(self, &store, owner, binding, range, &mut state, false).map(|_| ())
        } else {
            delivery::route(
                self,
                &store,
                owner,
                &binding,
                range,
                "Owner requested this transcript range",
                &mut state,
            )
        };
        if let Some(entry) = dispatch.owners.lock().get_mut(owner) {
            entry.running = false;
            entry.settle_route(result.clone());
            entry.dispatch_finished.notify_waiters();
        }
        self.emit_live_transcript_changed(owner);
        result
    }

    pub fn set_live_transcript_trigger(
        &self,
        owner: &str,
        trigger: TranscriptTrigger,
    ) -> Result<(), String> {
        let store = self.open_live_transcript(owner)?;
        let dispatch = self.ext_or_init(Dispatch::default);
        let mut owners = dispatch.owners.lock();
        let entry = owners.entry(owner.into()).or_default();
        if entry.running {
            return Err("A transcript dispatch is in progress".into());
        }
        let mut state = store.routing_state()?;
        state.trigger = trigger;
        store.save_routing_state(&state)?;
        entry.last_route_error = None;
        drop(owners);
        self.emit_live_transcript_changed(owner);
        Ok(())
    }
}

fn default_trigger(configured: &str, scout: ScoutStatus) -> TranscriptTrigger {
    serde_json::from_value::<TranscriptTrigger>(json!(configured))
        .ok()
        .filter(|trigger| {
            *trigger != TranscriptTrigger::ScoutThenConcierge || scout == ScoutStatus::Ready
        })
        .unwrap_or(if scout == ScoutStatus::Ready {
            TranscriptTrigger::ScoutThenConcierge
        } else {
            TranscriptTrigger::DirectBatch
        })
}

fn router_activity(ready: bool, turn_ms: Option<u64>, dispatching: bool) -> RouterActivity {
    if !ready {
        RouterActivity::Unavailable
    } else if transcript_router_stuck(turn_ms) {
        RouterActivity::Stuck
    } else if turn_ms.is_some() || dispatching {
        RouterActivity::Reviewing
    } else {
        RouterActivity::Idle
    }
}

impl TranscriptStore {
    fn routing_state(&self) -> Result<RoutingState, String> {
        read_private(&self.inner.root.join("routing.json"), MAX_RECORD_BYTES)
            .map(|state| state.unwrap_or_default())
            .map_err(|error| error.to_string())
    }

    fn save_routing_state(&self, state: &RoutingState) -> Result<(), String> {
        write_private(
            &self.inner.root.join("routing.json"),
            state,
            MAX_RECORD_BYTES,
        )
        .map_err(|error| error.to_string())
    }
}

pub(crate) fn is_silence_hallucination(text: &str) -> bool {
    let normalized = text
        .trim_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, '.' | ',' | '!' | '?' | '…' | '"' | '\'' | '«' | '»')
        })
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    matches!(
        normalized.as_str(),
        "" | "thank you"
            | "thank you very much"
            | "thanks"
            | "thanks for watching"
            | "thanks for listening"
            | "thank you for watching"
            | "thank you for listening"
            | "you"
            | "bye"
            | "goodbye"
            | "subtitles by"
            | "transcribed by"
            | "спасибо"
            | "спасибо за просмотр"
            | "продолжение следует"
            | "субтитры делал"
            | "субтитры подготовил"
    )
}

#[async_trait::async_trait]
impl LiveCaptureSink for CaptureSink {
    async fn prepare(&self, owner: &str) -> Result<(), String> {
        let svc = self.0.upgrade().ok_or("Live transcript service stopped")?;
        let owner = owner.to_owned();
        tokio::task::spawn_blocking(move || {
            let store = svc.open_live_transcript(&owner)?;
            store
                .verify_live_binding(&owner, &store.binding())
                .map_err(|error| error.to_string())?;
            super::maintenance::schedule(&svc, store);
            Ok(())
        })
        .await
        .map_err(|_| "Live transcript storage worker stopped".to_owned())?
    }

    async fn append(
        &self,
        capture: &LiveCaptureIdentity,
        event: LiveCaptureEvent,
    ) -> Result<(), String> {
        let svc = self.0.upgrade().ok_or("Live transcript service stopped")?;
        let owner = capture.tab_id.clone();
        let content = match event {
            LiveCaptureEvent::Speech { text } if is_silence_hallucination(&text) => return Ok(()),
            LiveCaptureEvent::Speech { text } => TranscriptContent::Speech { text },
            LiveCaptureEvent::Paused { reason } => TranscriptContent::Paused {
                reason: serde_json::to_value(reason)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default(),
            },
            LiveCaptureEvent::Resumed => TranscriptContent::Resumed,
        };
        tokio::task::spawn_blocking(move || {
            let store = svc.open_live_transcript(&owner)?;
            let receipt = store
                .append(
                    &owner,
                    TranscriptInput {
                        at_ms: wall_now_ms(),
                        content,
                    },
                )
                .map_err(|error| error.to_string())?;
            on_commit(&svc, &store, &owner, receipt);
            Ok(())
        })
        .await
        .map_err(|_| "Live transcript append worker stopped".to_owned())?
    }
}

fn on_commit(
    svc: &Arc<ServiceContext>,
    store: &TranscriptStore,
    owner: &str,
    receipt: AppendReceipt,
) {
    if !receipt.sealed.is_empty() {
        super::maintenance::schedule(svc, store.clone());
    }
    if matches!(receipt.row.content, TranscriptContent::Speech { .. }) {
        svc.emit_live_transcript_changed(owner);
        if store
            .routing_state()
            .is_ok_and(|state| state.trigger != TranscriptTrigger::Manual)
        {
            enqueue(svc, store, owner, receipt.row.seq);
        }
    }
}

fn watch_delivery(svc: &ServiceContext, store: TranscriptStore, owner: &str, send_id: &str) {
    let mut events = svc.events.subscribe();
    let svc = svc.weak_self();
    let owner = owner.to_owned();
    let send_id = send_id.to_owned();
    crate::async_runtime::spawn(async move {
        let finished = async {
            loop {
                match events.recv().await {
                    Ok(event)
                        if matches!(&event.event, codeterm_events::SystemEvent::Generic { event_type, data }
                        if event_type == "chat-send-status"
                        && data["send_id"].as_str() == Some(send_id.as_str())
                        && matches!(data["state"].as_str(), Some("delivered" | "failed"))) =>
                    {
                        break
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(60), finished).await;
        let Some(svc) = svc.upgrade() else { return };
        let _ = tokio::time::timeout(
            Duration::from_secs(60),
            settle_after_dispatch(svc, store, owner),
        )
        .await;
    });
}

pub(in crate::service) fn wake_when_idle(
    svc: &ServiceContext,
    store: TranscriptStore,
    owner: &str,
    destination: String,
    session: Arc<crate::service::agent_runtime::engine::Session>,
) {
    let mut events = svc.events.subscribe();
    let svc = svc.weak_self();
    let owner = owner.to_owned();
    // Settlement calls this while holding the dispatch lock, so the claim runs on the task.
    crate::async_runtime::spawn(async move {
        let claimed = svc.upgrade().is_some_and(|svc| {
            let mut owners = svc.ext_or_init(Dispatch::default).owners.lock();
            let entry = owners.entry(owner.clone()).or_default();
            !std::mem::replace(&mut entry.waiting_for_idle, true)
        });
        if !claimed {
            return;
        }
        let wait_idle = async {
            loop {
                let permit = session.gate.acquire().await;
                let turn_active = session.state.lock().active_turn_started_at_ms.is_some();
                let busy = turn_active || restart::model_restart_busy(&session);
                drop(permit);
                if !busy {
                    break;
                }
                match events.recv().await {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
                if svc
                    .upgrade()
                    .is_some_and(|s| s.mux.read().get_tab(&destination).is_none())
                {
                    break;
                }
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(90), wait_idle).await;
        let Some(svc) = svc.upgrade() else { return };
        {
            if let Some(entry) = svc
                .ext_or_init(Dispatch::default)
                .owners
                .lock()
                .get_mut(&owner)
            {
                entry.waiting_for_idle = false;
            }
        }
        svc.emit_live_transcript_changed(&owner);
        let committed = store.committed_seq();
        if committed > 0
            && store.routing_state().is_ok_and(|state| {
                state.trigger != TranscriptTrigger::Manual && committed > state.attempted_through
            })
        {
            enqueue(&svc, &store, &owner, committed);
        }
    });
}

async fn settle_after_dispatch(svc: Arc<ServiceContext>, store: TranscriptStore, owner: String) {
    loop {
        let result = tokio::task::spawn_blocking({
            let (svc, store, owner) = (svc.clone(), store.clone(), owner.clone());
            move || {
                let mut entries = svc.ext_or_init(Dispatch::default).owners.lock();
                if let Some(entry) = entries.get(&owner).filter(|entry| entry.running) {
                    return Some(entry.dispatch_finished.clone());
                }
                let result = store.routing_state().and_then(|mut state| {
                    delivery::settle(&svc, &store, &mut state)?;
                    let committed = store.committed_seq();
                    Ok((state.pending.is_none()
                        && state.trigger != TranscriptTrigger::Manual
                        && committed > state.attempted_through)
                        .then_some(committed))
                });
                if let Err(error) = &result {
                    entries
                        .entry(owner.clone())
                        .or_default()
                        .settle_route(Err(error.clone()));
                }
                drop(entries);
                svc.emit_live_transcript_changed(&owner);
                if let Ok(Some(committed)) = result {
                    enqueue(&svc, &store, &owner, committed);
                }
                None
            }
        })
        .await;
        let Ok(Some(finished)) = result else { return };
        let notified = finished.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let running = svc
            .ext_or_init(Dispatch::default)
            .owners
            .lock()
            .get(&owner)
            .is_some_and(|entry| entry.running);
        if running {
            notified.await;
        }
    }
}

fn enqueue(svc: &Arc<ServiceContext>, store: &TranscriptStore, owner: &str, seq: u64) {
    let dispatch = svc.ext_or_init(Dispatch::default);
    {
        let mut owners = dispatch.owners.lock();
        let entry = owners.entry(owner.into()).or_default();
        let pending = entry.pending.get_or_insert(TranscriptRange {
            from_seq: seq,
            to_seq: seq,
        });
        pending.to_seq = seq;
        if entry.running {
            return;
        }
        entry.running = true;
    }
    let svc = svc.clone();
    let store = store.clone();
    let owner = owner.to_owned();
    tokio::task::spawn_blocking(move || {
        let dispatch = svc.ext_or_init(Dispatch::default);
        loop {
            let range = {
                let mut owners = dispatch.owners.lock();
                let entry = owners.entry(owner.clone()).or_default();
                let Some(range) = entry.pending.take() else {
                    entry.running = false;
                    entry.dispatch_finished.notify_waiters();
                    break;
                };
                range
            };
            svc.emit_live_transcript_changed(&owner);
            let result = process(&svc, &store, &owner, range);
            if !matches!(&result, Ok(false)) {
                let mut owners = dispatch.owners.lock();
                owners
                    .entry(owner.clone())
                    .or_default()
                    .settle_route(result.map(|_| ()));
            }
            svc.emit_live_transcript_changed(&owner);
        }
        svc.emit_live_transcript_changed(&owner);
    });
}

fn process(
    svc: &ServiceContext,
    store: &TranscriptStore,
    owner: &str,
    range: TranscriptRange,
) -> Result<bool, String> {
    let mut state = store
        .routing_state()
        .map_err(|error| format!("routing state unavailable: {error}"))?;
    delivery::settle(svc, store, &mut state)
        .map_err(|error| format!("delivery proof unavailable: {error}"))?;
    if state.trigger == TranscriptTrigger::Manual
        || state.pending.is_some()
        || range.to_seq <= state.attempted_through
    {
        return Ok(false);
    }
    let observed_binding = store.binding();
    let local = matches!(observed_binding.target, super::BindingTarget::Local)
        || matches!(observed_binding.target, super::BindingTarget::Detached)
            && local::selected(svc, store);
    let attempted = if local {
        state.attempted_through.max(
            store
                .attached_cursor()
                .map_err(|error| error.to_string())?
                .acked_seq,
        )
    } else {
        state.attempted_through
    };
    let mut from_seq = attempted.saturating_add(1);
    if from_seq > range.to_seq {
        return Ok(false);
    }
    if local {
        return local::route_range(
            svc,
            store,
            owner,
            observed_binding,
            TranscriptRange {
                from_seq,
                to_seq: range.to_seq,
            },
            &mut state,
            true,
        );
    }
    let speech_owned: Vec<(u64, String)> = loop {
        let page = store
            .history(from_seq, 16)
            .map_err(|error| format!("committed speech unavailable: {error}"))?;
        let batch: Vec<(u64, String)> = page
            .rows
            .iter()
            .filter_map(|row| match &row.content {
                TranscriptContent::Speech { text } if row.seq <= range.to_seq => {
                    Some((row.seq, text.clone()))
                }
                _ => None,
            })
            .collect();
        if !batch.is_empty() {
            break batch;
        }
        let Some(last_seq) = page.rows.last().map(|row| row.seq) else {
            return Ok(false);
        };
        if last_seq >= range.to_seq {
            return Ok(false);
        }
        from_seq = last_seq.saturating_add(1);
    };
    let speech: Vec<(u64, &str)> = speech_owned
        .iter()
        .map(|(seq, text)| (*seq, text.as_str()))
        .collect();
    let Some(&(to_seq, _)) = speech.last() else {
        return Ok(false);
    };
    let range = TranscriptRange { from_seq, to_seq };
    let rationale = match state.trigger {
        TranscriptTrigger::ScoutThenConcierge => {
            if scout::classify(svc, owner, &speech)? != Some(true) {
                return Ok(false);
            }
            "Scout identified an explicit request"
        }
        TranscriptTrigger::WakeWord => {
            if !speech.iter().any(|(_, text)| wake_word(text)) {
                return Ok(false);
            }
            "Owner-selected wake word detected"
        }
        TranscriptTrigger::DirectBatch => "Owner-selected direct batch",
        TranscriptTrigger::Manual => return Ok(false),
    };
    delivery::route(
        svc,
        store,
        owner,
        &observed_binding,
        range,
        rationale,
        &mut state,
    )
    .map(|()| true)
}

fn wake_word(text: &str) -> bool {
    let mut words = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty());
    words
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("hey"))
        && words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("domios"))
}

#[cfg(test)]
mod load_tests;
#[cfg(test)]
mod local_tests;
#[cfg(test)]
mod tests;
