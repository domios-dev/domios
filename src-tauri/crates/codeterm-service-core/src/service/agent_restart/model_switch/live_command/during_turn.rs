use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScreenStage {
    Admission,
    Picker,
    Confirmation,
    AwaitingEvidence,
    UnknownPrompt,
}

pub(super) fn screen_stage(
    screen: &str,
    picker: Option<&CommandPicker>,
    prompt: Option<&CommandPrompt>,
    has_prompt: bool,
) -> ScreenStage {
    if picker.is_some_and(|picker| picker.present(screen)) {
        ScreenStage::Picker
    } else if prompt.is_some_and(|prompt| prompt.present(screen)) {
        ScreenStage::Confirmation
    } else if has_prompt {
        ScreenStage::UnknownPrompt
    } else {
        ScreenStage::AwaitingEvidence
    }
}

impl ServiceContext {
    pub(in crate::service::agent_restart::model_switch) fn during_turn_switch_incident(
        &self,
        snap: &TabRestartSnapshot,
        error: ModelSwitchError,
    ) -> ModelSwitchError {
        let error = match error {
            error @ ModelSwitchError::DuringTurnIncident { .. } => error,
            error => ModelSwitchError::DuringTurnIncident {
                stage: "admission".into(),
                screen: self
                    .screen_state
                    .get_screen_text(&snap.pty_id)
                    .unwrap_or_default(),
                reason: error.to_string(),
            },
        };
        let result = self.create_agent_notification_reported(
            "Mid-turn model switch failed".into(),
            Some(error.to_string()),
            Some("error"),
            None,
            Some(true),
            crate::service::notification::NotificationOrigin::local_tab(snap.tab_id.clone()),
            crate::service::notification::PushIntent {
                level: Some(codeterm_notification_core::contract::NotifyLevel::Failure),
                event_key: Some(format!(
                    "model-switch:{}:{}",
                    snap.tab_id,
                    uuid::Uuid::new_v4()
                )),
            },
            None,
        );
        if let Err(reason) = result {
            log::error!(
                "[agent-model] incident notification failed tab={}: {reason}",
                snap.tab_id
            );
        }
        error
    }
}

#[cfg(test)]
mod tests;
