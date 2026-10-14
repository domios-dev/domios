use super::{DecisionSource, LocalIdentity};
use crate::service::supporter::criteria_hash;
use crate::service::supporter_tab_state::{criteria, ObservationTrigger};
use codeterm_decision::{DecisionAnswer, DecisionResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Correlation {
    version: u8,
    decision_point: String,
    producer: Producer,
    parent_observation_id: Option<u64>,
    observation_id: u64,
    launch_id: String,
    session_id: Option<String>,
    occurrence_id: String,
    occurrence: u64,
    frame_hash: String,
    candidate_evidence: CandidateEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Producer {
    LifecycleEvent,
    OutputCandidate,
    ParkedEdge,
    PromptEstablished,
    PromptConfirmation,
    SentinelRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum CandidateEvidence {
    Observed {
        source: CandidateSource,
        kind: CandidateKind,
    },
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CandidateSource {
    ProviderSafeTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CandidateKind {
    Update,
    Onboarding,
}

impl Correlation {
    pub(crate) fn decision_point(&self) -> &str {
        &self.decision_point
    }

    pub(crate) fn launch_id(&self) -> &str {
        &self.launch_id
    }

    pub(crate) fn occurrence_id(&self) -> &str {
        &self.occurrence_id
    }

    pub(crate) fn is_safe_response_producer(&self) -> bool {
        matches!(
            self.producer,
            Producer::OutputCandidate
                | Producer::ParkedEdge
                | Producer::PromptEstablished
                | Producer::PromptConfirmation
                | Producer::SentinelRequest
        )
    }

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
    ) -> Self {
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
        Self {
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ModelVerdict {
    Observed { label: String, confidence: f64 },
    Unavailable,
}

impl ModelVerdict {
    fn observed(label: &str, confidence: Option<f64>) -> Option<Self> {
        let confidence = confidence.filter(|p| p.is_finite() && (0.0..=1.0).contains(p))?;
        criteria().contains_key(label).then(|| Self::Observed {
            label: label.into(),
            confidence,
        })
    }

    pub(crate) fn from_response(response: &DecisionResponse) -> Option<Self> {
        let DecisionAnswer::Choice {
            choice,
            probabilities,
            confidence,
        } = response.answers.get("label")?
        else {
            return None;
        };
        Self::observed(
            choice,
            confidence.or_else(|| probabilities.get(choice).copied()),
        )
    }

    fn valid(&self) -> bool {
        match self {
            Self::Observed { label, confidence } => {
                Self::observed(label, Some(*confidence)).is_some()
            }
            Self::Unavailable => true,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum AlternateVerdict {
    Skipped,
    Unavailable,
}

fn hash(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn supported_decision_point(point: &str) -> bool {
    matches!(
        point,
        "launch_ready"
            | "startup_deadline"
            | "startup_escalation"
            | "prompt_candidate"
            | "worker_idle_without_report"
            | "busy_without_output"
            | "tab_exit"
            | "user_delivery_readiness"
    )
}

pub(crate) fn project(value: &Value) -> Option<Value> {
    let correlation: Correlation = serde_json::from_value(value["correlation"].clone()).ok()?;
    if correlation.version != 2
        || Some(correlation.observation_id) != value["observation_id"].as_u64()
        || Some(correlation.decision_point.as_str()) != value["decision_point"].as_str()
        || !supported_decision_point(&correlation.decision_point)
        || match correlation.producer {
            Producer::PromptEstablished | Producer::PromptConfirmation => {
                correlation.decision_point != "prompt_candidate"
                    || correlation
                        .parent_observation_id
                        .is_none_or(|parent| parent == 0 || parent == correlation.observation_id)
            }
            Producer::OutputCandidate | Producer::ParkedEdge => {
                correlation.decision_point != "prompt_candidate"
                    || correlation.parent_observation_id.is_some()
            }
            Producer::SentinelRequest => {
                correlation.decision_point != "prompt_candidate"
                    || correlation
                        .parent_observation_id
                        .is_some_and(|parent| parent == 0 || parent == correlation.observation_id)
            }
            Producer::LifecycleEvent => correlation.parent_observation_id.is_some(),
        }
        || !hash(&correlation.launch_id, 64)
        || !hash(&correlation.occurrence_id, 64)
        || correlation
            .session_id
            .as_ref()
            .is_some_and(|session| !hash(session, 64))
        || !hash(&correlation.frame_hash, 16)
    {
        return None;
    }
    serde_json::to_value(correlation).ok()
}

pub(crate) fn project_diagnostics(value: &Value, entry: &mut Value) {
    let correlation = project(value);
    let decision_point = value["decision_point"]
        .as_str()
        .filter(|point| supported_decision_point(point));
    if let Some(correlation) = correlation.as_ref() {
        entry["decision_point"] = correlation["decision_point"].clone();
        entry["correlation"] = correlation.clone();
    } else if let Some(decision_point) = decision_point {
        entry["decision_point"] = json!(decision_point);
    }
    if let Some(status) = value["observation_status"]
        .as_str()
        .filter(|status| ["completed", "rejected", "unavailable"].contains(status))
    {
        entry["observation_status"] = json!(if correlation.is_none() {
            "unavailable"
        } else {
            status
        });
    }
    if correlation.is_some() || decision_point.is_some() {
        // Action phases need a valid correlation; classification phases may stand alone.
        let action_allowed = correlation.is_some();
        if let Some(phase) = value["phase"].as_str().filter(|phase| {
            ["classification_completed", "classification_rejected"].contains(phase)
                || (action_allowed && ["action_authorized", "action_stale"].contains(phase))
        }) {
            entry["phase"] = json!(phase);
        }
    }
    if let Some(verdict) = value.get("model_verdict") {
        let verdict = serde_json::from_value::<ModelVerdict>(verdict.clone())
            .ok()
            .filter(ModelVerdict::valid)
            .unwrap_or(ModelVerdict::Unavailable);
        entry["model_verdict"] = json!(verdict);
    }
    if let Some(verdict) = value.get("alternate_verdict") {
        let verdict = serde_json::from_value::<AlternateVerdict>(verdict.clone())
            .unwrap_or(AlternateVerdict::Unavailable);
        entry["alternate_verdict"] = json!(verdict);
    }
}

#[cfg(test)]
mod tests;
