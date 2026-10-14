use super::chat::CapturedChat;
use super::leases::{Capture, FrozenTarget};
use crate::service::agent_pad::ChatSendResult;
use crate::service::chat_send_transaction::ChatSendStatus;
use crate::service::ServiceContext;
use serde::Serialize;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LocalRecordingCommitOutcome {
    Tracked { status: ChatSendStatus },
    PreHandoffFailure { message: String },
    PossibleHandoff { message: String },
}

impl ServiceContext {
    pub(crate) fn freeze_recording_target(&self, tab_id: &str) -> Result<FrozenTarget, String> {
        if self.tab_chat_backing(tab_id).is_some() {
            let identity = self
                .captured_chat_identity(tab_id)
                .ok_or("recording_recipient_not_authenticated_ready")?;
            self.chatbackend_session_info(&identity.plugin_id, &identity.session_id)?;
            return Ok(FrozenTarget::Chat(identity));
        }
        let authority = self.user_send_authority(tab_id)?;
        let session = authority
            .session_id
            .as_deref()
            .ok_or("recording_recipient_not_authenticated_ready")?;
        if authority.prelaunch
            || !self.user_authority_is_current_session(
                &authority,
                session,
                &authority.binding_nonce,
            )
            || !(self.latch_user_authority_ready(&authority)
                || self.user_authority_is_ready(&authority))
        {
            return Err("recording_recipient_not_authenticated_ready".into());
        }
        Ok(FrozenTarget::Pty(authority))
    }

    pub(crate) fn validate_recording_target(&self, capture: &Capture) -> Result<(), String> {
        capture.validate()?;
        self.require_managed_send_allowed(capture.target.tab_id(), None)?;
        if self.freeze_recording_target(capture.target.tab_id())? != capture.target {
            return Err("recording_recipient_changed".into());
        }
        Ok(())
    }

    pub(crate) async fn local_recording_commit(
        &self,
        lease: &str,
        clip_handle: &str,
        client_token: String,
    ) -> LocalRecordingCommitOutcome {
        let (capture, path) = match self.local_recordings.transcribing_clip(lease, clip_handle) {
            Ok(value) => value,
            Err(error) => {
                self.local_recordings
                    .finish_clip_pre_handoff(lease, clip_handle);
                self.local_recordings.cancel(lease);
                return LocalRecordingCommitOutcome::PreHandoffFailure { message: error };
            }
        };
        let path_text = match path.to_str() {
            Some(path) => path,
            None => {
                self.local_recordings
                    .finish_clip_pre_handoff(lease, clip_handle);
                self.local_recordings.cancel(lease);
                return LocalRecordingCommitOutcome::PreHandoffFailure {
                    message: "recording_audio_path_invalid".into(),
                };
            }
        };
        let active_id = self.configuration().integrations.voice_transcribe.active;
        let original = super::completion::complete_voice_capture(
            &capture,
            path_text,
            &self.transcribe_registry,
            &active_id,
            || self.validate_recording_target(&capture),
        )
        .await;
        let original = match original {
            Ok(frame) => frame,
            Err(message) => {
                self.local_recordings
                    .finish_clip_pre_handoff(lease, clip_handle);
                self.local_recordings.cancel(lease);
                return LocalRecordingCommitOutcome::PreHandoffFailure { message };
            }
        };
        if let Err(message) = self
            .local_recordings
            .mark_clip_handoff_possible(lease, clip_handle)
        {
            self.local_recordings
                .finish_clip_pre_handoff(lease, clip_handle);
            self.local_recordings.cancel(lease);
            return LocalRecordingCommitOutcome::PreHandoffFailure { message };
        }
        let svc = self.clone();
        let capture_for_send = capture.clone();
        let admission = tokio::task::spawn_blocking(move || {
            svc.admit_recording_voice(capture_for_send, original, client_token)
        })
        .await;
        match admission {
            Ok(Ok(result)) => {
                let status = result.status;
                let _ = self
                    .local_recordings
                    .retain_clip_after_handoff(lease, clip_handle);
                self.local_recordings.finish(lease);
                return LocalRecordingCommitOutcome::Tracked { status };
            }
            Ok(Err(message)) if message.starts_with("admission_unknown:") => {
                let _ = self
                    .local_recordings
                    .retain_clip_after_handoff(lease, clip_handle);
                self.local_recordings.cancel(lease);
                return LocalRecordingCommitOutcome::PossibleHandoff { message };
            }
            Ok(Err(message)) => {
                self.local_recordings
                    .finish_clip_pre_handoff(lease, clip_handle);
                self.local_recordings.cancel(lease);
                LocalRecordingCommitOutcome::PreHandoffFailure { message }
            }
            Err(error) => {
                let message = format!("admission_unknown: worker outcome uncertain: {error}");
                let _ = self
                    .local_recordings
                    .retain_clip_after_handoff(lease, clip_handle);
                self.local_recordings.cancel(lease);
                LocalRecordingCommitOutcome::PossibleHandoff { message }
            }
        }
    }

    pub(crate) fn admit_recording_voice(
        &self,
        capture: Capture,
        original: String,
        client_token: String,
    ) -> Result<ChatSendResult, String> {
        self.validate_recording_target(&capture)?;
        let result = match &capture.target {
            FrozenTarget::Pty(expected) => {
                let reservation =
                    self.reserve_user_send(expected.tab_id.as_str(), Some(client_token))?;
                if &reservation.authority != expected {
                    return Err("recording_reservation_changed".into());
                }
                capture.validate()?;
                self.prepare_and_admit_recording_user_send(reservation, original, capture)
            }
            FrozenTarget::Chat(identity) => {
                let (text, pad_file) =
                    self.prepare_chat_send_text(&identity.tab_id, original, true)?;
                self.validate_recording_target(&capture)?;
                let status = self.start_captured_chat_send(
                    CapturedChat {
                        identity: identity.clone(),
                        active: capture.active.clone(),
                        expires_at: *capture.expires_at.lock(),
                    },
                    text,
                    client_token,
                )?;
                Ok(ChatSendResult::new(status, pad_file))
            }
        };
        if result.is_ok() {
            self.record_product_feature(codeterm_analytics::Feature::VoiceInputUsed);
        }
        result
    }
}
