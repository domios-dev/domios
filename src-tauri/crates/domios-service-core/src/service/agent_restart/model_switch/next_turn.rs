use super::*;

impl ServiceContext {
    pub(super) fn switch_model_on_next_turn(
        &self,
        snap: &TabRestartSnapshot,
        session_id: &str,
        requested: &str,
        effort: Option<&str>,
        previous_model: Option<String>,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        let boundary = self
            .agent_registry
            .with_provider(&snap.provider, |provider| {
                provider.session_model_transcript_boundary(
                    snap.cwd.as_deref().unwrap_or_default(),
                    session_id,
                )
            })
            .flatten()
            .filter(|boundary| boundary.boundary_found)
            .ok_or_else(|| {
                ModelSwitchError::NotSwitchable(
                    "next-turn switch requires an own-session transcript boundary".into(),
                )
            })?;
        self.record_model_request(
            &snap.tab_id,
            &snap.provider,
            session_id,
            requested,
            effort,
            Some(boundary),
        )?;
        Ok(ModelSwitchOutcome {
            tab_id: snap.tab_id.clone(),
            provider: snap.provider.clone(),
            session_id: session_id.into(),
            restart: None,
            mechanism: ModelSwitchMode::NextTurn,
            fallback_reason: None,
            previous_model,
            requested_model: requested.into(),
            launched_model: requested.into(),
            confirmation_detail: None,
            confirmation: ModelSwitchConfirmation::Pending,
        })
    }
}

pub(super) fn record_pending_turn(
    tab: &mut crate::multiplexer::types::Tab,
    session_id: &str,
    requested: &str,
    boundary: codeterm_types::agent::SessionModelTranscriptBoundary,
) {
    tab.pending_model_switch_observation =
        Some(crate::multiplexer::types::PendingModelSwitchObservation {
            session_id: session_id.into(),
            launched_model: requested.into(),
            transcript_boundary: boundary,
        });
}

#[cfg(test)]
mod tests;
