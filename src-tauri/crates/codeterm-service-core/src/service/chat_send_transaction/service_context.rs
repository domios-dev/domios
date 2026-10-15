use super::*;
use tauri::Manager;

impl crate::service::ServiceContext {
    pub fn start_chat_send(
        &self,
        tab_id: &str,
        text: String,
        submit_claim: Option<crate::service::pty_io::submit_transaction::SubmitCycleClaim>,
        client_token: Option<String>,
    ) -> Result<ChatSendStatus, String> {
        self.start_chat_send_with_capture(tab_id, text, submit_claim, client_token, None)
    }

    pub(crate) fn start_captured_chat_send(
        &self,
        capture: super::super::local_recording::chat::CapturedChat,
        text: String,
        client_token: String,
    ) -> Result<ChatSendStatus, String> {
        let tab_id = capture.identity.tab_id.clone();
        self.start_chat_send_with_capture(&tab_id, text, None, Some(client_token), Some(capture))
    }

    fn start_chat_send_with_capture(
        &self,
        tab_id: &str,
        text: String,
        submit_claim: Option<crate::service::pty_io::submit_transaction::SubmitCycleClaim>,
        client_token: Option<String>,
        capture: Option<super::super::local_recording::chat::CapturedChat>,
    ) -> Result<ChatSendStatus, String> {
        self.wake_frozen_tab(tab_id)?;
        if let Some(capture) = &capture {
            capture
                .validate(self.captured_chat_identity(tab_id))
                .map_err(|error| error.ledger_message())?;
        }
        if super::super::agent_runtime::requested_for_tab(self, tab_id) {
            if let Some(capture) = capture {
                return super::super::agent_runtime::capture::admit(
                    self,
                    capture,
                    &text,
                    client_token,
                );
            }
        }
        if self.tab_chat_backing(tab_id).is_none()
            || super::super::agent_runtime::requested_for_tab(self, tab_id)
        {
            if capture.is_some() {
                return Err("recording_recipient_changed".into());
            }
            drop(submit_claim);
            let reservation = self.reserve_user_send(tab_id, client_token)?;
            return self
                .admit_prepared_user_send(reservation, &text, &text, None)
                .map(|(status, _)| status);
        }
        let evidence = self.chat_send_anchor(tab_id);
        let turn_evidence_text = self.chat_send_turn_evidence(tab_id, &text);
        if let Some(capture) = &capture {
            capture
                .validate(self.captured_chat_identity(tab_id))
                .map_err(|error| error.ledger_message())?;
            if evidence.live_binding.as_ref().is_none_or(|binding| {
                binding.session_id.as_deref() != Some(capture.identity.session_id.as_str())
                    || binding.binding_nonce.as_deref()
                        != Some(capture.identity.binding_nonce.as_str())
            }) {
                return Err("recording_recipient_evidence_changed".into());
            }
        }
        let pending = self.chat_send_ledger.try_start_scoped_with_normalization(
            tab_id,
            &turn_evidence_text,
            evidence.anchor.clone(),
            evidence.live_binding.clone(),
            evidence.turn_normalization,
        )?;
        if let Some(deferred) = evidence.deferred_first_turn.clone() {
            self.chat_send_ledger
                .set_deferred_first_turn_evidence(&pending.send_id, deferred);
        }
        let pending = match client_token {
            Some(token) => self
                .chat_send_ledger
                .attach_client_token(&pending.send_id, token)
                .unwrap_or(pending),
            None => pending,
        };
        let response = pending.clone();
        let send_id = pending.send_id.clone();
        let svc = self.clone();
        let tab_id = tab_id.to_string();
        std::thread::spawn(move || {
            let status = run_chat_send(&svc.chat_send_ledger, pending, &text, |text| {
                if let Some(capture) = &capture {
                    return svc.chatbackend_capture_send(capture, text);
                }
                // User-origin always-deliver: a transient pre-write unlink or an
                // unconfirmed submit degrades to pending-until-link, never a refusal.
                svc.send_chat_text_claimed(&tab_id, text, submit_claim)
                    .or_else(|err| {
                        log::info!(
                            "[chat-send] user send to Tab {tab_id} degraded to pending ({})",
                            err.code()
                        );
                        svc.park_refused_user_send(&tab_id, &send_id, text, &err);
                        err.into_surfaced(crate::service::delivery_error::SendOrigin::User)
                    })
            });
            if let Some(capture) = &capture {
                capture
                    .active
                    .store(false, std::sync::atomic::Ordering::Release);
            }
            codeterm_events::emit_everywhere(&svc.app_handle, "chat-send-status", &status);
            if let Some(error) = status.error.as_deref() {
                log::warn!(
                    "[chat-send] send_id={} tab={} failed: {}",
                    status.send_id,
                    status.tab_id,
                    error
                );
            }
        });
        self.record_product_activity();
        Ok(response)
    }

    pub(crate) fn reserve_user_send(
        &self,
        tab_id: &str,
        client_token: Option<String>,
    ) -> Result<UserReservation, String> {
        self.reserve_user_send_with_replay_only(tab_id, client_token, false)
    }

    pub fn reserve_user_send_with_replay_only(
        &self,
        tab_id: &str,
        client_token: Option<String>,
        replay_only: bool,
    ) -> Result<UserReservation, String> {
        self.chat_send_ledger
            .restore_before_user_admission(tab_id, || self.restore_durable_user_statuses(tab_id))?;
        let authority = self.user_send_authority(tab_id)?;
        if let Some(service) = self.app_handle.try_state::<super::super::ServiceState>() {
            let weak = Arc::downgrade(service.inner());
            *self.chat_send_ledger.owner.lock() = Some(weak.clone());
            let tab = tab_id.to_string();
            self.pending_delivery_payloads
                .user_sends
                .set_admission_wake(
                    tab_id,
                    Arc::new(move || {
                        if let Some(service) = weak.upgrade() {
                            service.wake_user_owner(
                                &tab,
                                super::super::parked_user_delivery::UserWake::Housekeeping,
                            );
                        }
                    }),
                );
        }
        // This short reservation is the service ordering point, before asynchronous preparation.
        self.pending_delivery_payloads
            .user_sends
            .reserve_with_replay_only(authority, client_token, replay_only, || {
                self.chat_send_ledger.reserve_identity()
            })
    }

    pub fn prepare_and_admit_user_send(
        &self,
        reservation: UserReservation,
        original: String,
        run_stages: bool,
    ) -> Result<super::super::agent_pad::ChatSendResult, String> {
        self.prepare_and_admit_user_send_with_capture(reservation, original, run_stages, None)
    }

    pub(crate) fn prepare_and_admit_recording_user_send(
        &self,
        reservation: UserReservation,
        original: String,
        capture: super::super::local_recording::leases::Capture,
    ) -> Result<super::super::agent_pad::ChatSendResult, String> {
        self.prepare_and_admit_user_send_with_capture(reservation, original, true, Some(capture))
    }

    fn prepare_and_admit_user_send_with_capture(
        &self,
        reservation: UserReservation,
        original: String,
        run_stages: bool,
        capture: Option<super::super::local_recording::leases::Capture>,
    ) -> Result<super::super::agent_pad::ChatSendResult, String> {
        if reservation.wait_existing()? {
            let status = self
                .chat_send_ledger
                .status(&reservation.send_id)
                .ok_or("admission_unknown: original token has no retained ledger result")?;
            return Ok(super::super::agent_pad::ChatSendResult::new(status, None));
        }
        if original.len() > MAX_PREPARED_BYTES {
            return Err("user original exceeds 4 MiB storage bound".into());
        }
        let (prepared, returned) = self.prepare_chat_send_text(
            &reservation.authority.tab_id,
            original.clone(),
            run_stages,
        )?;
        let body = returned
            .as_ref()
            .map(|path| self.capture_prepared_user_body(&reservation.authority.tab_id, path))
            .transpose()?;
        if let Some(capture) = &capture {
            self.validate_recording_target(capture)?;
            if capture.target
                != super::super::local_recording::leases::FrozenTarget::Pty(
                    reservation.authority.clone(),
                )
            {
                return Err("recording_reservation_changed".into());
            }
            self.local_recordings.register_write_ticket(
                &reservation.send_id,
                &capture.lease_id,
                capture.clone(),
            )?;
        }
        let send_id = reservation.send_id.clone();
        let result = self.admit_prepared_user_send(reservation, &original, &prepared, body);
        if result
            .as_ref()
            .err()
            .is_some_and(|error| !error.starts_with("admission_unknown:"))
        {
            self.local_recordings.retire_write_ticket(&send_id);
        }
        result.map(|(status, owned)| super::super::agent_pad::ChatSendResult::new(status, owned))
    }

    fn capture_prepared_user_body(
        &self,
        tab_id: &str,
        returned: &std::path::Path,
    ) -> Result<PreparedUserBody, String> {
        let pad_root = self.agent_pad_data_dir.join("runtime").join("agent-pads");
        let pad = pad_root.join(tab_id);
        let inbox = pad.join("inbox");
        if returned.parent() != Some(inbox.as_path())
            || !std::fs::symlink_metadata(&pad)
                .map_err(|_| "prepared tab scope unavailable")?
                .file_type()
                .is_dir()
            || !std::fs::symlink_metadata(&inbox)
                .map_err(|_| "prepared inbox scope unavailable")?
                .file_type()
                .is_dir()
        {
            return Err("prepared body does not belong to the exact returned tab inbox".into());
        }
        let root = pad_root
            .canonicalize()
            .map_err(|_| "prepared root scope unavailable")?;
        let canonical_pad = pad
            .canonicalize()
            .map_err(|_| "prepared tab scope unavailable")?;
        if canonical_pad.parent() != Some(root.as_path()) {
            return Err("prepared tab escaped its owner scope".into());
        }
        let expected = inbox
            .canonicalize()
            .map_err(|_| "prepared inbox scope unavailable")?;
        if expected.parent() != Some(canonical_pad.as_path()) {
            return Err("prepared inbox escaped its tab scope".into());
        }
        if !std::fs::symlink_metadata(returned)
            .map_err(|_| "prepared body unavailable")?
            .file_type()
            .is_file()
        {
            return Err("prepared body is not an owned regular file".into());
        }
        let actual = returned
            .canonicalize()
            .map_err(|_| "prepared body scope unavailable")?;
        if actual.parent() != Some(expected.as_path()) {
            return Err("prepared body escaped the returned inbox scope".into());
        }
        let bytes = super::super::pending_delivery_payloads::user_sends::read_bounded(
            &actual,
            MAX_PREPARED_BYTES as u64,
        )?;
        Ok(PreparedUserBody {
            returned_path: returned.to_path_buf(),
            bytes,
        })
    }

    fn admit_prepared_user_send(
        &self,
        reservation: UserReservation,
        original: &str,
        prepared: &str,
        body: Option<PreparedUserBody>,
    ) -> Result<(ChatSendStatus, Option<std::path::PathBuf>), String> {
        if reservation.wait_existing()? {
            let status = self
                .chat_send_ledger
                .status(&reservation.send_id)
                .ok_or("admission_unknown: original token has no retained ledger result")?;
            return Ok((status, None));
        }
        let store = &self.pending_delivery_payloads.user_sends;
        let final_transport = store.final_transport(&reservation, prepared, body.as_ref())?;
        let follow_up = self
            .agent_registry
            .with_provider(&reservation.authority.provider_id, |provider| {
                if provider.agent_runtime().is_some() {
                    return None;
                }
                provider.paste_capability().and_then(|capability| {
                    capability
                        .typed_follow_up_for(&final_transport)
                        .map(str::to_string)
                })
            })
            .flatten();
        let held_before_readiness = !self.user_authority_is_ready(&reservation.authority);
        let record = store.admit_with_follow_up(
            &reservation,
            original,
            prepared,
            body,
            follow_up,
            held_before_readiness,
        )?;
        let status = self.chat_send_ledger.admit_user(&record);
        self.record_product_activity();
        let mut status = if held_before_readiness {
            self.chat_send_ledger
                .set_user_pending_reason(&record.send_id, Some("awaiting_agent_ready"))
                .unwrap_or(status)
        } else {
            status
        };
        let path = store
            .body_path(&record)?
            .map(|path| {
                path.to_str()
                    .map(|text| {
                        std::path::PathBuf::from(codeterm_paths::strip_verbatim_prefix(text))
                    })
                    .ok_or_else(|| {
                        "admission_unknown: durable body path cannot be represented".to_string()
                    })
            })
            .transpose()?;
        codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        let tab = record.authority.tab_id.clone();
        if self.user_authority_is_ready(&record.authority) {
            if let Some(pty_id) = record.authority.pty_id.as_deref() {
                let claim = super::super::pty_io::submit_transaction::claim_submit_cycle(pty_id);
                if let Some(wait) = claim.command_wait() {
                    status = self
                        .hold_user_send_for_model_command(&tab, &record.send_id, wait)
                        .unwrap_or(status);
                } else {
                    self.chat_send_ledger
                        .store_user_submit_claim(&record.send_id, claim);
                }
            }
        }
        drop(reservation);
        if !record.held_before_readiness && self.user_authority_is_ready(&record.authority) {
            self.dispatch_ready_user_send(&record);
        } else {
            self.wake_user_owner(
                &tab,
                super::super::parked_user_delivery::UserWake::Admission,
            );
        }
        Ok((status, path))
    }

    pub(crate) fn admit_runtime_question_answer(
        &self,
        reservation: UserReservation,
        input: codeterm_plugin::agent_runtime::TurnInput,
        resolution: super::super::pending_delivery_payloads::user_sends::runtime_questions::QuestionResolution,
    ) -> Result<ChatSendStatus, String> {
        if !input.attachments.is_empty() {
            return Err("Question answer formatting cannot create attachments".into());
        }
        if reservation.wait_existing()? {
            return self
                .chat_send_ledger
                .status(&reservation.send_id)
                .ok_or_else(|| "Original question answer receipt unavailable".into());
        }
        let held = !self.user_authority_is_ready(&reservation.authority);
        let record = self
            .pending_delivery_payloads
            .user_sends
            .admit_question_answer(&reservation, &input.text, resolution, held)?;
        let status = self.chat_send_ledger.admit_user(&record);
        let tab = record.authority.tab_id.clone();
        codeterm_events::emit_everywhere(&self.app_handle, "chat-send-status", &status);
        drop(reservation);
        self.record_product_activity();
        self.wake_user_owner(
            &tab,
            super::super::parked_user_delivery::UserWake::Admission,
        );
        Ok(status)
    }

    pub(crate) fn chat_send_turn_evidence(&self, tab_id: &str, text: &str) -> String {
        let provider_id = self.mux.read().get_tab(tab_id).and_then(|tab| {
            matches!(&tab.backing, crate::multiplexer::types::TabBacking::Pty)
                .then(|| tab.agent_type.clone())
                .flatten()
        });
        let Some(provider_id) = provider_id else {
            return text.to_string();
        };
        let sanitized = codeterm_pty::pty::ansi::sanitize_agent_prompt_input(text);
        let follow_up = self
            .agent_registry
            .with_provider(&provider_id, |provider| {
                provider
                    .paste_capability()?
                    .typed_follow_up_for(&sanitized)
                    .map(str::to_string)
            })
            .flatten();
        follow_up.map_or_else(
            || text.to_string(),
            |follow_up| format!("{sanitized}{follow_up}"),
        )
    }
}
