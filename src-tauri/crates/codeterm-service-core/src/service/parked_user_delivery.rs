use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};

fn take_parked_user_send(
    pending: &mut Option<PendingDeliveryRecord>,
    record_id: &str,
    send_id: Option<&str>,
) -> Option<PendingDeliveryRecord> {
    let record = pending.as_ref()?;
    if record.channel != PendingDeliveryChannel::InteractiveInput
        || record.record_id() != record_id
        || record.send_id() != send_id
        || record.state() != PendingDeliveryState::Pending
    {
        return None;
    }
    pending.take()
}

fn return_after_send_failure(
    pending: &mut Option<PendingDeliveryRecord>,
    record: PendingDeliveryRecord,
    bytes_written: bool,
) -> Result<(), PendingDeliveryRecord> {
    if bytes_written
        || pending
            .as_ref()
            .is_some_and(|current| current.record_id() != record.record_id())
    {
        return Err(record);
    }
    *pending = Some(record);
    Ok(())
}

use super::delivery_error::DeliveryError;
use super::pending_delivery_payloads::user_sends::{
    UserSendAuthority, UserSendRecord, UserSendState,
};
use std::sync::atomic::Ordering;

pub(crate) enum UserDrain {
    Idle,
    ZeroWriteRefusal { dispatch_generation: u64 },
}

type UserReadiness = super::supporter_tab_state::UserDeliveryReadiness;

mod continuity;
mod drain;
mod handoff;
use handoff::{PayloadEnqueued, UserWrite};
mod owner_wake;
use owner_wake::OwnerWakeBoundary;

fn is_prelaunch_stage(stage: Option<codeterm_types::pty::BootStage>) -> bool {
    matches!(
        stage,
        None | Some(
            codeterm_types::pty::BootStage::PtyStarting
                | codeterm_types::pty::BootStage::PtySpawned
                | codeterm_types::pty::BootStage::ReaderReady
                | codeterm_types::pty::BootStage::ShellReady
                | codeterm_types::pty::BootStage::AgentLaunching
        )
    )
}

fn is_launch_written_or_later(stage: Option<codeterm_types::pty::BootStage>) -> bool {
    matches!(
        stage,
        Some(
            codeterm_types::pty::BootStage::LaunchWritten
                | codeterm_types::pty::BootStage::ProviderReady
                | codeterm_types::pty::BootStage::PreparingRoleInstructions
                | codeterm_types::pty::BootStage::DeliveringInstructions
                | codeterm_types::pty::BootStage::AgentReady
        )
    )
}

fn prelaunch_authority_matches_current_launch(
    tab: &crate::multiplexer::types::Tab,
    authority: &UserSendAuthority,
) -> bool {
    let same_pty = tab.pty_id.as_deref() == authority.pty_id.as_deref();
    let same_provider = tab.agent_type.as_deref() == Some(authority.provider_id.as_str());
    let same_nonce = tab.session_binding_nonce.as_deref() == Some(authority.binding_nonce.as_str());
    let nonce_was_just_rotated = tab
        .prior_session_binding_nonces
        .last()
        .is_some_and(|prior| prior.nonce == authority.binding_nonce);
    authority.prelaunch
        && same_provider
        && (authority.pty_id.is_none() || same_pty)
        && tab.boot_stage != Some(codeterm_types::pty::BootStage::SpawnFailed)
        && (is_prelaunch_stage(tab.boot_stage) || is_launch_written_or_later(tab.boot_stage))
        && (same_nonce || nonce_was_just_rotated)
}

fn prelaunch_authority_matches_failed_launch(
    tab: &crate::multiplexer::types::Tab,
    authority: &UserSendAuthority,
) -> bool {
    let same_pty = tab.pty_id.as_deref() == authority.pty_id.as_deref();
    let same_provider = tab.agent_type.as_deref() == Some(authority.provider_id.as_str());
    let same_nonce = tab.session_binding_nonce.as_deref() == Some(authority.binding_nonce.as_str());
    let nonce_was_just_rotated = tab
        .prior_session_binding_nonces
        .last()
        .is_some_and(|prior| prior.nonce == authority.binding_nonce);
    authority.prelaunch
        && same_provider
        && (authority.pty_id.is_none() || same_pty)
        && tab.boot_stage == Some(codeterm_types::pty::BootStage::SpawnFailed)
        && (same_nonce || nonce_was_just_rotated)
}

fn requires_observed_session_id_for_readiness(
    observed_session_id: Option<&str>,
    session_starts_on_first_user_turn: bool,
) -> bool {
    observed_session_id.is_none() && !session_starts_on_first_user_turn
}

fn supporter_pending_reason(error: &str) -> &'static str {
    match error {
        "supporter_adapter_unavailable" => "supporter_adapter_unavailable",
        "supporter_observation_not_queued" => "supporter_observation_not_queued",
        "supporter_observation_deadline" => "supporter_observation_deadline",
        "supporter_observation_cancelled" => "supporter_observation_cancelled",
        "supporter_observation_unavailable" => "supporter_observation_unavailable",
        "supporter_pty_generation_changed_during_capture" => "supporter_pty_generation_changed",
        "user delivery PTY session generation unavailable" => "pty_session_generation_unavailable",
        _ => "supporter_observation_capture_unavailable",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum UserWake {
    Readiness,
    Admission,
    Housekeeping,
}

impl super::ServiceContext {
    pub fn user_send_authority(&self, tab_id: &str) -> Result<UserSendAuthority, String> {
        let runtime = super::agent_runtime::requested_for_tab(self, tab_id);
        let (pty_id, binding_nonce, provider_id, session_id, observed_session_id, prelaunch) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("user send tab is absent")?;
            if !runtime && !matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty) {
                return Err("user send owner requires PTY backing".into());
            }
            let session_id = tab.agent_session_id.clone();
            (
                tab.pty_id.clone(),
                tab.session_binding_nonce
                    .clone()
                    .ok_or("user send has no launch nonce")?,
                tab.agent_type
                    .clone()
                    .ok_or("user send has no agent identity")?,
                tab.launch_session_authenticated()
                    .then(|| session_id.clone())
                    .flatten(),
                session_id,
                (tab.pty_id.is_none() || tab.agent_booting) && is_prelaunch_stage(tab.boot_stage),
            )
        };
        let pty_session_generation = pty_id.as_deref().and_then(|pty_id| {
            self.pty
                .lock()
                .get_session(pty_id)
                .map(|session| session.generation)
        });
        Ok(UserSendAuthority {
            tab_id: tab_id.to_string(),
            pty_id,
            binding_nonce,
            pty_session_generation,
            provider_id,
            session_id,
            observed_session_id,
            prelaunch,
        })
    }

    pub fn user_authority_is_ready(&self, authority: &UserSendAuthority) -> bool {
        if super::agent_runtime::requested_for_tab(self, &authority.tab_id) {
            return super::agent_runtime::user_delivery::is_ready(self, authority);
        }
        let owner = self
            .pending_delivery_payloads
            .user_sends
            .tab(&authority.tab_id);
        if owner.revoked.load(Ordering::Acquire) || owner.failed.load(Ordering::Acquire) {
            return false;
        }
        if !self.user_authority_pty_generation_is_current(authority) {
            return false;
        }
        if !self
            .mux
            .read()
            .get_tab(&authority.tab_id)
            .is_some_and(|tab| self.user_authority_matches_launch(tab, authority))
        {
            return false;
        }
        self.pending_delivery_payloads
            .user_sends
            .ready_launch_is_latched(authority)
    }

    pub(crate) fn latch_user_authority_ready(&self, authority: &UserSendAuthority) -> bool {
        if !self.user_authority_pty_generation_is_current(authority) {
            return false;
        }
        let is_current_agent_ready =
            self.mux
                .read()
                .get_tab(&authority.tab_id)
                .is_some_and(|tab| {
                    self.user_authority_matches_launch(tab, authority)
                        && tab.boot_stage == Some(codeterm_types::pty::BootStage::AgentReady)
                });
        is_current_agent_ready && self.latch_user_input_authority(authority)
    }

    pub(crate) fn latch_user_input_authority(&self, authority: &UserSendAuthority) -> bool {
        self.user_authority_matches_launch_current(authority)
            && self
                .mux
                .read()
                .get_tab(&authority.tab_id)
                .is_some_and(|tab| super::boot_stage::session_allows_user_readiness(tab))
            && !self
                .pending_delivery_payloads
                .user_sends
                .tab(&authority.tab_id)
                .failed
                .load(Ordering::Acquire)
            && self.rearm_user_input_for_authority(authority)
            && self
                .pending_delivery_payloads
                .user_sends
                .latch_ready_launch(authority)
    }

    fn rearm_user_input_for_authority(&self, authority: &UserSendAuthority) -> bool {
        let Some(pty_id) = authority.pty_id.as_deref() else {
            return false;
        };
        let Some(generation) = authority.pty_session_generation else {
            return false;
        };
        let pty = self.pty.lock();
        pty.session_generation_matches(pty_id, generation)
            && pty.rearm_guarded_input(pty_id).is_ok()
    }

    pub(crate) fn user_authority_is_current_session(
        &self,
        authority: &UserSendAuthority,
        session_id: &str,
        binding_nonce: &str,
    ) -> bool {
        if super::agent_runtime::requested_for_tab(self, &authority.tab_id) {
            return super::agent_runtime::user_delivery::is_current_session(
                self,
                authority,
                session_id,
                binding_nonce,
            );
        }
        self.user_authority_pty_generation_is_current(authority)
            && self
                .mux
                .read()
                .get_tab(&authority.tab_id)
                .is_some_and(|tab| {
                    self.user_authority_matches_launch(tab, authority)
                        && tab.launch_session_authenticated()
                        && tab.agent_session_id.as_deref() == Some(session_id)
                        && tab.reported_session_binding_nonce.as_deref() == Some(binding_nonce)
                        && authority.binding_nonce == binding_nonce
                })
    }

    fn user_authority_matches_launch(
        &self,
        tab: &crate::multiplexer::types::Tab,
        authority: &UserSendAuthority,
    ) -> bool {
        matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty)
            && tab.pty_id.as_deref() == authority.pty_id.as_deref()
            && tab.session_binding_nonce.as_deref() == Some(authority.binding_nonce.as_str())
            && tab.agent_type.as_deref() == Some(authority.provider_id.as_str())
    }

    fn user_authority_is_from_ended_launch(&self, authority: &UserSendAuthority) -> bool {
        let mux = self.mux.read();
        let Some(tab) = mux.get_tab(&authority.tab_id) else {
            return false;
        };
        let same_pty = tab.pty_id.as_deref() == authority.pty_id.as_deref();
        let same_provider = tab.agent_type.as_deref() == Some(authority.provider_id.as_str());
        let in_flight_prelaunch = prelaunch_authority_matches_current_launch(tab, authority);
        let session_changed =
            super::pending_delivery_payloads::user_sends::continuity::known_session_changed(
                authority.observed_session_id.as_deref(),
                tab.agent_session_id.as_deref(),
            );
        session_changed
            || prelaunch_authority_matches_failed_launch(tab, authority)
            || (!in_flight_prelaunch
                && (!same_pty
                    || tab.session_binding_nonce.as_deref()
                        != Some(authority.binding_nonce.as_str())
                    || !same_provider))
    }

    fn user_authority_pty_generation_is_current(&self, authority: &UserSendAuthority) -> bool {
        let (Some(pty_id), Some(generation)) = (
            authority.pty_id.as_deref(),
            authority.pty_session_generation,
        ) else {
            return false;
        };
        self.pty
            .lock()
            .session_generation_matches(pty_id, generation)
    }

    fn advance_prelaunch_user_authority(
        &self,
        record: &UserSendRecord,
    ) -> Result<Option<UserSendRecord>, String> {
        if !record.authority.prelaunch {
            return Ok(None);
        }
        let stage = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&record.authority.tab_id) else {
                return Ok(None);
            };
            if !matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty)
                || !prelaunch_authority_matches_current_launch(tab, &record.authority)
            {
                return Ok(None);
            }
            tab.boot_stage
        };
        let current = self.user_send_authority(&record.authority.tab_id)?;
        let Some(pty_id) = current.pty_id.as_deref() else {
            return Ok(None);
        };
        let Some(session) = self.pty.lock().get_session(pty_id) else {
            return Ok(None);
        };
        let mut authority = current;
        authority.pty_session_generation = Some(session.generation);
        authority.prelaunch = !is_launch_written_or_later(stage);
        let still_current = {
            let mux = self.mux.read();
            mux.get_tab(&record.authority.tab_id).is_some_and(|tab| {
                prelaunch_authority_matches_current_launch(tab, &record.authority)
                    && self.user_authority_matches_launch(tab, &authority)
            })
        };
        if !still_current {
            return Ok(None);
        }
        if authority == record.authority {
            return Ok(None);
        }
        let Some(rebound) = self
            .pending_delivery_payloads
            .user_sends
            .rebind_queued_authority(record, authority.clone())?
        else {
            return Ok(None);
        };
        if !self.chat_send_ledger.rebind_queued_user_authority(
            &record.send_id,
            &record.authority,
            &authority,
        ) {
            return Err(
                "durable user authority changed without a matching ledger admission".into(),
            );
        }
        Ok(Some(rebound))
    }

    fn pending_user_send(&self, record: &UserSendRecord, reason: &str) -> UserDrain {
        log::warn!(
            "[chat-send] user owner pending tab={} send_id={} reason={reason}",
            record.authority.tab_id,
            record.send_id,
        );
        if let Some(status) = self
            .chat_send_ledger
            .set_user_pending_reason(&record.send_id, Some(reason))
        {
            codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
        UserDrain::Idle
    }

    fn clear_user_pending_reason(&self, record: &UserSendRecord) {
        if let Some(status) = self
            .chat_send_ledger
            .set_user_pending_reason(&record.send_id, None)
        {
            codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
    }

    fn user_readiness_fallback(
        &self,
        authority: &UserSendAuthority,
    ) -> Result<UserReadiness, &'static str> {
        let (same_launch, binding_authenticated) = {
            let mux = self.mux.read();
            mux.get_tab(&authority.tab_id)
                .map_or((false, false), |tab| {
                    (
                        self.user_authority_matches_launch(tab, authority),
                        tab.launch_session_authenticated(),
                    )
                })
        };
        if !same_launch {
            return Err("launch_generation_changed");
        }
        if !self.user_authority_pty_generation_is_current(authority) {
            return Err("pty_session_generation_unavailable_or_changed");
        }
        if binding_authenticated {
            return Err("exact_binding_present_stage_pending");
        }
        let session_starts_on_first_user_turn = self
            .agent_registry
            .with_provider(&authority.provider_id, |provider| {
                provider.session_starts_on_first_user_turn()
            })
            .unwrap_or(false);
        if session_starts_on_first_user_turn {
            return Err("provider_composer_pending");
        }
        if requires_observed_session_id_for_readiness(
            authority.observed_session_id.as_deref(),
            session_starts_on_first_user_turn,
        ) {
            return Err("resume_session_unknown");
        }
        let first = self
            .observe_user_delivery_readiness(&authority.tab_id, None)
            .map_err(|error| supporter_pending_reason(&error))?;
        if let Some(reason) = first.pending_reason() {
            return Err(reason);
        }
        if !self.user_delivery_observation_is_current(&first) {
            return Err("supporter_first_observation_stale");
        }
        let second = self
            .observe_user_delivery_readiness(&authority.tab_id, Some(&first))
            .map_err(|error| supporter_pending_reason(&error))?;
        let pair = UserReadiness::new(first, second).map_err(|reason| reason)?;
        let current_launch = {
            let mux = self.mux.read();
            mux.get_tab(&authority.tab_id)
                .is_some_and(|tab| self.user_authority_matches_launch(tab, authority))
        };
        if !current_launch {
            return Err("launch_generation_changed");
        }
        if !self.user_delivery_observation_is_current(pair.current_observation()) {
            return Err("supporter_second_observation_stale");
        }
        Ok(pair)
    }

    pub(crate) fn park_refused_user_send(
        &self,
        tab_id: &str,
        send_id: &str,
        text: &str,
        error: &DeliveryError,
    ) {
        if error.bytes_written() || text.is_empty() {
            return;
        }
        let result = (|| {
            let accepted = self
                .chat_send_ledger
                .status(send_id)
                .ok_or("user send ledger admission absent")?;
            let reservation = self
                .pending_delivery_payloads
                .user_sends
                .reserve_with_replay_only(
                    self.user_send_authority(tab_id)?,
                    accepted.correlation_id,
                    false,
                    || (send_id.to_string(), accepted.sequence),
                )?;
            let record =
                self.pending_delivery_payloads
                    .user_sends
                    .admit(&reservation, text, text, None)?;
            self.chat_send_ledger.admit_user(&record);
            if !self.user_authority_is_ready(&record.authority) {
                self.chat_send_ledger
                    .set_user_pending_reason(&record.send_id, Some("awaiting_agent_ready"));
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            log::warn!("[chat-send] tab={tab_id} admission held: {error}");
        }
    }

    pub(crate) fn wake_parked_user_send(&self, tab_id: &str) {
        self.wake_user_owner(tab_id, UserWake::Readiness);
    }

    pub(crate) fn wake_user_owner_on_state_change(&self, tab_id: &str) {
        if self
            .pending_delivery_payloads
            .user_sends
            .has_unsettled(tab_id)
        {
            self.wake_user_owner(tab_id, UserWake::Readiness);
        }
    }

    pub(crate) fn wake_user_owner(&self, tab_id: &str, reason: UserWake) {
        let owner = self.pending_delivery_payloads.user_sends.tab(tab_id);
        if reason != UserWake::Housekeeping {
            let generation = owner.dispatch_generation.fetch_add(1, Ordering::AcqRel) + 1;
            owner.dispatch_events.send_replace(generation);
        }
        owner.wake_generation.fetch_add(1, Ordering::AcqRel);
        if owner.revoked.load(Ordering::Acquire)
            || owner.waiting_for_dispatch.load(Ordering::Acquire)
        {
            return;
        }
        self.start_user_owner(tab_id, owner);
    }

    fn start_user_owner(
        &self,
        tab_id: &str,
        owner: std::sync::Arc<super::pending_delivery_payloads::user_sends::UserTab>,
    ) {
        if owner.revoked.load(Ordering::Acquire)
            || owner
                .running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let service = self.clone();
        let tab_id = tab_id.to_string();
        let dispatch_events = owner.dispatch_events.subscribe();
        tauri::async_runtime::spawn_blocking(move || {
            loop {
                let generation = owner.wake_generation.load(Ordering::Acquire);
                let dispatch_generation = owner.dispatch_generation.load(Ordering::Acquire);
                let outcome = service.drain_user_owner(&tab_id);
                if let Err(error) = &outcome {
                    log::warn!("[chat-send] tab={tab_id} owner held: {error}");
                }
                let boundary =
                    OwnerWakeBoundary::after_drain(generation, dispatch_generation, &outcome);
                let refusal_generation = boundary.refusal_generation();
                let changed = || {
                    boundary.changed(
                        owner.wake_generation.load(Ordering::Acquire),
                        owner.dispatch_generation.load(Ordering::Acquire),
                    )
                };
                if !owner.revoked.load(Ordering::Acquire) && changed() {
                    continue;
                }
                if let Some(refused_at) = refusal_generation {
                    let mut events = dispatch_events.clone();
                    owner.waiting_for_dispatch.store(true, Ordering::Release);
                    owner.running.store(false, Ordering::Release);
                    if owner.revoked.load(Ordering::Acquire) {
                        owner.waiting_for_dispatch.store(false, Ordering::Release);
                        break;
                    }
                    if owner.dispatch_generation.load(Ordering::Acquire) != refused_at {
                        owner.waiting_for_dispatch.store(false, Ordering::Release);
                        if owner
                            .running
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                        {
                            continue;
                        }
                        break;
                    }
                    let service = service.clone();
                    let tab_id = tab_id.clone();
                    let wake_owner = owner.clone();
                    tauri::async_runtime::spawn(async move {
                        while !wake_owner.revoked.load(Ordering::Acquire)
                            && wake_owner.dispatch_generation.load(Ordering::Acquire) == refused_at
                        {
                            if events.changed().await.is_err() {
                                break;
                            }
                        }
                        wake_owner
                            .waiting_for_dispatch
                            .store(false, Ordering::Release);
                        if !wake_owner.revoked.load(Ordering::Acquire) {
                            service.start_user_owner(&tab_id, wake_owner);
                        }
                    });
                    break;
                }
                owner.running.store(false, Ordering::Release);
                if owner.revoked.load(Ordering::Acquire)
                    || !changed()
                    || owner
                        .running
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                {
                    break;
                }
            }
            #[cfg(test)]
            tests::notify_owner_idle(&tab_id);
        });
    }

    pub(crate) fn deliver_parked_user_send(&self, tab_id: &str) {
        self.wake_parked_user_send(tab_id);
    }

    fn user_readiness_is_current(
        &self,
        authority: &UserSendAuthority,
        readiness: &UserReadiness,
    ) -> bool {
        self.user_authority_matches_launch_current(authority)
            && self.user_authority_pty_generation_is_current(authority)
            && self.user_delivery_observation_is_current(readiness.current_observation())
    }

    fn user_send_readiness_is_current(
        &self,
        authority: &UserSendAuthority,
        readiness: Option<&UserReadiness>,
    ) -> bool {
        self.user_authority_is_ready(authority)
            || readiness
                .is_some_and(|readiness| self.user_readiness_is_current(authority, readiness))
    }

    fn user_authority_matches_launch_current(&self, authority: &UserSendAuthority) -> bool {
        let owner = self
            .pending_delivery_payloads
            .user_sends
            .tab(&authority.tab_id);
        !owner.revoked.load(Ordering::Acquire)
            && !owner.failed.load(Ordering::Acquire)
            && self.user_authority_pty_generation_is_current(authority)
            && self
                .mux
                .read()
                .get_tab(&authority.tab_id)
                .is_some_and(|tab| self.user_authority_matches_launch(tab, authority))
    }

    fn submit_owned_user(
        &self,
        record: &UserSendRecord,
        text: &str,
        readiness: Option<&UserReadiness>,
        capture_send: bool,
    ) -> Result<UserWrite, DeliveryError> {
        let service = self.clone();
        let completion_service = self.clone();
        let send_id = record.send_id.clone();
        let owned_record = record.clone();
        let text = text.to_owned();
        let readiness = readiness.cloned();
        self.run_write_handoff(
            &record.authority.tab_id,
            move |enqueued| {
                service.submit_owned_user_transaction(
                    &owned_record,
                    &text,
                    readiness.as_ref(),
                    capture_send,
                    enqueued,
                )
            },
            move |outcome| completion_service.finish_user_write(&send_id, outcome),
        )
    }

    fn finish_user_write(&self, send_id: &str, outcome: Result<(), DeliveryError>) {
        if let Some(status) = self
            .chat_send_ledger
            .finish(send_id, outcome.map_err(|error| error.ledger_message()))
        {
            codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
    }

    fn submit_owned_user_transaction(
        &self,
        record: &UserSendRecord,
        text: &str,
        readiness: Option<&UserReadiness>,
        capture_send: bool,
        enqueued: PayloadEnqueued,
    ) -> Result<(), DeliveryError> {
        let authority = &record.authority;
        let tab_id = authority.tab_id.as_str();
        let admitted_claim = self
            .chat_send_ledger
            .take_user_submit_claim(&record.send_id);
        if !self.user_send_readiness_is_current(authority, readiness) {
            return Err(DeliveryError::TabBusyRefused {
                tab: tab_id.into(),
                detail: "user owner is not ready for this launch".into(),
            });
        }
        if !self.rearm_user_input_for_authority(authority) {
            return Err(DeliveryError::TabBusyRefused {
                tab: tab_id.into(),
                detail: "user owner readiness changed before guarded input".into(),
            });
        }
        if let Some(result) = super::codex_rpc::send::try_send_owned(self, record, capture_send) {
            if capture_send
                && result
                    .as_ref()
                    .err()
                    .is_some_and(|error| !error.bytes_written())
            {
                self.pending_user_send(record, "recording_capture_rpc_refused_before_handoff");
            }
            return result;
        }
        let pty_id = authority
            .pty_id
            .as_deref()
            .ok_or_else(|| DeliveryError::PtyUnavailable {
                tab: tab_id.into(),
                detail: "user owner has no resolved PTY authority".into(),
            })?;
        let claim = admitted_claim
            .unwrap_or_else(|| super::pty_io::submit_transaction::claim_submit_cycle(pty_id))
            .with_trace_id(&record.send_id);
        if let Some(wait) = claim.command_wait() {
            self.hold_user_send_for_model_command(tab_id, &record.send_id, wait);
            return Err(DeliveryError::TabBusyRefused {
                tab: tab_id.into(),
                detail: "model command owns PTY input".into(),
            });
        }
        // Resolved after readiness was observed, so a hook installed before readiness is never missed.
        let permit = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or_else(|| DeliveryError::TabBusyRefused {
                    tab: tab_id.into(),
                    detail: "user owner tab closed".into(),
                })?;
            if tab.pty_id.as_deref() != Some(pty_id)
                || authority.pty_session_generation.is_none_or(|generation| {
                    !self
                        .pty
                        .lock()
                        .session_generation_matches(pty_id, generation)
                })
                || tab.session_binding_nonce.as_deref() != Some(authority.binding_nonce.as_str())
                || tab.agent_type.as_deref() != Some(authority.provider_id.as_str())
                || self
                    .pending_delivery_payloads
                    .user_sends
                    .tab(tab_id)
                    .revoked
                    .load(Ordering::Acquire)
            {
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.into(),
                    detail: "user owner launch changed".into(),
                });
            }
            self.pty
                .lock()
                .issue_guarded_input_permit(pty_id)
                .map_err(|detail| DeliveryError::PtyUnavailable {
                    tab: tab_id.into(),
                    detail,
                })?
        };
        let strategy = self
            .agent_registry
            .with_provider(&authority.provider_id, |provider| {
                provider.verify_submit_strategy()
            })
            .ok_or_else(|| DeliveryError::TabBusyRefused {
                tab: tab_id.into(),
                detail: "user owner provider unavailable".into(),
            })?;
        let track_turn = !text.is_empty()
            && matches!(
                &strategy,
                crate::agent::provider::VerifySubmitStrategy::TuiInputDrained { .. }
            );
        if !self.user_send_readiness_is_current(authority, readiness) {
            return Err(DeliveryError::TabBusyRefused {
                tab: tab_id.into(),
                detail: "user owner revoked before submit cycle".into(),
            });
        }
        if !self.user_send_readiness_is_current(authority, readiness) {
            return Err(DeliveryError::TabBusyRefused {
                tab: tab_id.into(),
                detail: "user owner readiness changed before payload write".into(),
            });
        }
        let service = self.clone();
        let guard_authority = authority.clone();
        let pending_record = record.clone();
        let readiness = readiness.cloned().map(std::sync::Arc::new);
        let pre_write_guard: std::sync::Arc<dyn Fn() -> bool + Send + Sync> =
            std::sync::Arc::new(move || {
                let capture_current = !capture_send
                    || service
                        .local_recordings
                        .write_ticket_matches_authority(&pending_record.send_id, &guard_authority);
                let current = capture_current
                    && service
                        .user_send_readiness_is_current(&guard_authority, readiness.as_deref());
                if !current {
                    service.pending_user_send(
                        &pending_record,
                        if capture_current {
                            "readiness_changed_before_payload_write"
                        } else {
                            "recording_capture_invalid_before_payload_write"
                        },
                    );
                }
                current
            });
        let write_service = self.clone();
        let write_record = record.clone();
        let payload_queued: PayloadEnqueued = std::sync::Arc::new(move || {
            write_service.mark_user_send_written(&write_record);
            enqueued();
        });
        #[cfg(test)]
        if let Some(boundary) = tests::queued_boundary::boundary(tab_id) {
            if !pre_write_guard() {
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.into(),
                    detail: "user owner authority changed before payload write".into(),
                });
            }
            return boundary(Some(record), text, payload_queued);
        }
        #[cfg(test)]
        if let Some(boundary) = tests::submit_boundary(tab_id) {
            tests::run_before_submit(tab_id);
            if !pre_write_guard() {
                return Err(DeliveryError::TabBusyRefused {
                    tab: tab_id.into(),
                    detail: "user owner authority changed before payload write".into(),
                });
            }
            let outcome = boundary(record, text);
            if outcome
                .as_ref()
                .err()
                .is_none_or(DeliveryError::bytes_written)
            {
                payload_queued();
            }
            return outcome;
        }
        super::pty_io::submit_transaction::submit_text_with_key_claimed_and_follow_up_revalidated(
            self,
            pty_id,
            text,
            record.typed_follow_up.as_deref(),
            true,
            b"\r",
            strategy,
            super::inject_verify::VerifyOpts {
                require_prompt_first: false,
                verify_echo: true,
                verify_submit: true,
                ..Default::default()
            },
            Some(permit),
            track_turn,
            claim,
            Some(pre_write_guard),
            true,
            Some(payload_queued),
        )
        .and_then(|result| {
            let elapsed = result.elapsed;
            result
                .require_submission(track_turn, elapsed, String::new())
                .map(|_| ())
        })
        .map_err(|error| DeliveryError::from_injection(tab_id, error))
    }

    fn mark_user_send_written(&self, record: &UserSendRecord) {
        if let Some(status) = self.chat_send_ledger.mark_user_written(&record.send_id) {
            codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        }
    }
}

mod bootstrap;
mod degraded;
#[cfg(test)]
pub(crate) mod tests;
