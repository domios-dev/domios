use super::*;
use crate::agent::provider::ModelSwitchMode;
use crate::multiplexer::types::PendingModelSwitchObservation;
use codeterm_types::agent::SessionModelTranscriptBoundary;

fn retain_next_turn(mode: ModelSwitchMode, target_matches: bool) -> bool {
    mode == ModelSwitchMode::NextTurn && !target_matches
}

fn advance_boundary(
    current: &mut Option<PendingModelSwitchObservation>,
    expected: &PendingModelSwitchObservation,
    boundary: Option<SessionModelTranscriptBoundary>,
) {
    if current.as_ref() != Some(expected) {
        return;
    }
    if let Some(boundary) = boundary.filter(|boundary| boundary.boundary_found) {
        if let Some(current) = current.as_mut() {
            current.transcript_boundary = boundary;
        }
    }
}

impl ServiceContext {
    pub(super) fn retain_unapplied_next_turn(
        &self,
        tab_id: &str,
        agent_id: &str,
        cwd: &str,
        observation: &PendingModelSwitchObservation,
        binding_nonce: &str,
        observed_model: &str,
    ) -> bool {
        let mode = self
            .agent_registry
            .with_provider(agent_id, |provider| provider.model_switch())
            .unwrap_or(ModelSwitchMode::Unsupported);
        let target_matches = session_model_matches(
            &self.agent_registry,
            agent_id,
            &observation.launched_model,
            observed_model,
        );
        if !retain_next_turn(mode, target_matches) {
            return false;
        }
        let boundary = self
            .agent_registry
            .with_provider(agent_id, |provider| {
                provider.session_model_transcript_boundary(cwd, &observation.session_id)
            })
            .flatten();
        let mut mux = self.mux.write();
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            if runtime_model_update_matches_binding(
                tab,
                agent_id,
                &observation.session_id,
                Some(binding_nonce),
            ) {
                advance_boundary(
                    &mut tab.pending_model_switch_observation,
                    observation,
                    boundary,
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boundary(id: &str) -> SessionModelTranscriptBoundary {
        SessionModelTranscriptBoundary {
            last_entry_uuid: Some(id.into()),
            last_entry_timestamp: None,
            last_entry_type: None,
            boundary_found: true,
        }
    }

    fn observation() -> PendingModelSwitchObservation {
        PendingModelSwitchObservation {
            session_id: "own-session".into(),
            launched_model: "target/model".into(),
            transcript_boundary: boundary("before-fallback"),
        }
    }

    #[test]
    fn only_unapplied_next_turn_selections_remain_pending() {
        assert!(retain_next_turn(ModelSwitchMode::NextTurn, false));
        assert!(!retain_next_turn(ModelSwitchMode::NextTurn, true));
        assert!(!retain_next_turn(ModelSwitchMode::RestartResume, false));
        assert!(!retain_next_turn(ModelSwitchMode::LiveCommand, false));
        assert!(!retain_next_turn(ModelSwitchMode::Live, false));
    }

    #[test]
    fn old_model_turn_advances_evidence_window_and_retains_target() {
        let expected = observation();
        let mut pending = Some(expected.clone());
        advance_boundary(
            &mut pending,
            &expected,
            Some(boundary("completed-fallback")),
        );
        let pending = pending.unwrap();
        assert_eq!(pending.launched_model, expected.launched_model);
        assert_eq!(pending.session_id, expected.session_id);
        assert_eq!(
            pending.transcript_boundary.last_entry_uuid.as_deref(),
            Some("completed-fallback")
        );
    }

    #[test]
    fn unavailable_boundary_keeps_selection_and_prior_evidence_window() {
        let expected = observation();
        let mut pending = Some(expected.clone());
        advance_boundary(&mut pending, &expected, None);
        assert_eq!(pending, Some(expected));
    }

    #[test]
    fn superseded_selection_cannot_be_rearmed_by_an_old_observation() {
        let expected = observation();
        let mut replacement = expected.clone();
        replacement.launched_model = "other/model".into();
        let mut pending = Some(replacement.clone());
        advance_boundary(
            &mut pending,
            &expected,
            Some(boundary("completed-fallback")),
        );
        assert_eq!(pending, Some(replacement));
    }
}
