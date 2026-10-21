use super::{DecisionSource, LocalIdentity};
use crate::service::supporter::criteria_hash;
use crate::service::supporter_tab_state::ObservationTrigger;
pub(crate) use codeterm_supporter::diagnostics::*;
use serde_json::{json, Value};
pub(crate) fn capture(
    local: Option<&LocalIdentity>,
    generation: &str,
    point: ObservationTrigger,
    producer: Producer,
    parent_observation_id: Option<u64>,
    occurrence: u64,
    observation_id: u64,
    frame_hash: u64,
    candidate_kind: Option<&str>,
) -> Correlation {
    let launch_id = criteria_hash(
        local
            .map_or(generation, |local| &local.launch_key)
            .as_bytes(),
    );
    let session_id = local
        .and_then(|local| local.session.as_deref())
        .map(|session| criteria_hash(session.as_bytes()));
    let occurrence_id = criteria_hash(
        json!([launch_id, session_id, occurrence])
            .to_string()
            .as_bytes(),
    );
    let kind = match candidate_kind {
        Some("update") => Some(CandidateKind::Update),
        Some("onboarding") => Some(CandidateKind::Onboarding),
        _ => None,
    };
    Correlation {
        version: 2,
        decision_point: point.label().into(),
        producer,
        parent_observation_id,
        observation_id,
        launch_id,
        session_id,
        occurrence_id,
        occurrence,
        frame_hash: format!("{frame_hash:016x}"),
        candidate_evidence: kind.map_or(CandidateEvidence::Unavailable, |kind| {
            CandidateEvidence::Observed {
                source: CandidateSource::ProviderSafeTarget,
                kind,
            }
        }),
    }
}
pub(crate) struct RecordIdentity<'a> {
    pub(crate) observation_id: u64,
    correlation: Option<&'a Correlation>,
    fallback_decision_point: Option<&'a str>,
    model_verdict: Option<ModelVerdict>,
}

impl From<u64> for RecordIdentity<'_> {
    fn from(observation_id: u64) -> Self {
        Self::with_correlation(observation_id, None)
    }
}

impl<'a> RecordIdentity<'a> {
    pub(crate) fn captured(observation_id: u64, correlation: &'a Correlation) -> Self {
        Self::with_correlation(observation_id, Some(correlation))
    }

    pub(crate) fn with_correlation(
        observation_id: u64,
        correlation: Option<&'a Correlation>,
    ) -> Self {
        Self {
            observation_id,
            correlation,
            fallback_decision_point: None,
            model_verdict: None,
        }
    }

    pub(crate) fn from_metadata(
        observation_id: u64,
        metadata: &'a crate::service::supporter_tab_state::TabStateMetadata,
    ) -> Self {
        Self::with_correlation(
            observation_id,
            metadata
                .envelope
                .as_ref()
                .map(|envelope| &envelope.correlation),
        )
        .with_fallback_decision_point(
            metadata.context["observation_event"]
                .as_str()
                .unwrap_or_default(),
        )
    }

    pub(crate) fn with_fallback_decision_point(mut self, decision_point: &'a str) -> Self {
        self.fallback_decision_point = Some(decision_point);
        self
    }

    pub(crate) fn with_model_verdict(mut self, verdict: Option<ModelVerdict>) -> Self {
        self.model_verdict = verdict;
        self
    }

    pub(crate) fn decision_point(&self) -> Option<&str> {
        self.correlation
            .filter(|correlation| correlation.observation_id == self.observation_id)
            .map(Correlation::decision_point)
            .or_else(|| {
                self.fallback_decision_point
                    .filter(|point| supported_decision_point(point))
            })
    }

    pub(super) fn decorate(
        self,
        event: &mut Value,
        source: DecisionSource,
        label: &str,
        confidence: Option<f64>,
    ) {
        let captured = self
            .correlation
            .filter(|correlation| correlation.observation_id == self.observation_id);
        let completed = matches!(
            source,
            DecisionSource::Adapter | DecisionSource::Other | DecisionSource::LowConfidence
        );
        event["observation_status"] = json!(if captured.is_none() {
            "unavailable"
        } else if completed {
            "completed"
        } else {
            "rejected"
        });
        event["phase"] = json!(if completed {
            "classification_completed"
        } else {
            "classification_rejected"
        });
        if let Some(correlation) = captured {
            event["decision_point"] = json!(correlation.decision_point);
            event["correlation"] = json!(correlation);
        } else if let Some(decision_point) = self
            .fallback_decision_point
            .filter(|point| supported_decision_point(point))
        {
            event["decision_point"] = json!(decision_point);
        }
        let verdict = self
            .model_verdict
            .or_else(|| {
                if completed {
                    ModelVerdict::observed(label, confidence)
                } else {
                    None
                }
            })
            .unwrap_or(ModelVerdict::Unavailable);
        event["model_verdict"] = json!(verdict);
        event["alternate_verdict"] = json!(if source == DecisionSource::Adapter {
            AlternateVerdict::Skipped
        } else {
            AlternateVerdict::Unavailable
        });
    }
}

#[cfg(test)]
mod tests;
