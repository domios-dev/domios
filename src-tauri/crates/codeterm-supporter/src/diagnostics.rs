use codeterm_decision::{DecisionAnswer, DecisionResponse};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Correlation {
    pub version: u8,
    pub decision_point: String,
    pub producer: Producer,
    pub parent_observation_id: Option<u64>,
    pub observation_id: u64,
    pub launch_id: String,
    pub session_id: Option<String>,
    pub occurrence_id: String,
    pub occurrence: u64,
    pub frame_hash: String,
    pub candidate_evidence: CandidateEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Producer {
    LifecycleEvent,
    OutputCandidate,
    ParkedEdge,
    PromptEstablished,
    PromptConfirmation,
    SentinelRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CandidateEvidence {
    Observed {
        source: CandidateSource,
        kind: CandidateKind,
    },
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSource {
    ProviderSafeTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Update,
    Onboarding,
}

impl Correlation {
    pub fn decision_point(&self) -> &str {
        &self.decision_point
    }

    pub fn launch_id(&self) -> &str {
        &self.launch_id
    }

    pub fn occurrence_id(&self) -> &str {
        &self.occurrence_id
    }

    pub fn is_safe_response_producer(&self) -> bool {
        matches!(
            self.producer,
            Producer::OutputCandidate
                | Producer::ParkedEdge
                | Producer::PromptEstablished
                | Producer::PromptConfirmation
                | Producer::SentinelRequest
        )
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelVerdict {
    Observed { label: String, confidence: f64 },
    Unavailable,
}

impl ModelVerdict {
    pub fn observed(label: &str, confidence: Option<f64>) -> Option<Self> {
        let confidence = confidence.filter(|p| p.is_finite() && (0.0..=1.0).contains(p))?;
        criteria().contains_key(label).then(|| Self::Observed {
            label: label.into(),
            confidence,
        })
    }

    pub fn from_response(response: &DecisionResponse) -> Option<Self> {
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
pub enum AlternateVerdict {
    Skipped,
    Unavailable,
}

fn hash(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn supported_decision_point(point: &str) -> bool {
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

pub fn project(value: &Value) -> Option<Value> {
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

pub fn project_diagnostics(value: &Value, entry: &mut Value) {
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
fn criteria() -> &'static std::collections::BTreeMap<String, String> {
    static CRITERIA: std::sync::OnceLock<std::collections::BTreeMap<String, String>> =
        std::sync::OnceLock::new();
    CRITERIA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../evals/tab-state-jev/criteria.json"
        ))
        .expect("embedded tab-state criteria are valid")
    })
}

#[cfg(test)]
mod tests;
