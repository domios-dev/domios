use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod deadline;
pub mod physical;

pub trait DecisionModel: Send + Sync {
    fn supports_calibrated_choice_confidence(&self) -> bool {
        false
    }
    /// Batches all named questions into at most one physical request.
    /// Transports account each attempt through `physical::before_request`.
    fn supports_question_batch(&self) -> bool {
        false
    }
    fn id(&self) -> &str;
    fn token_mode(&self) -> TokenMode;
    fn model_id(&self) -> Option<String> {
        None
    }
    fn pricing(&self) -> Option<DecisionPricing> {
        None
    }
    fn metadata(&self) -> DecisionMetadata {
        DecisionMetadata::default()
    }
    fn primitives(&self) -> DecisionPrimitives;
    fn models(&self) -> Vec<DecisionModelInfo> {
        Vec::new()
    }
    fn select_model(&self, id: &str) -> bool {
        self.model_id().as_deref() == Some(id) && self.models().iter().any(|model| model.id == id)
    }
    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        deadline: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        decide_questions_sequentially(self, request, deadline)
    }
    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionMetadata {
    pub display_name: Option<String>,
    pub help_url: Option<String>,
    pub server_address: Option<String>,
    pub settings_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenMode {
    Required,
    Optional,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionModelInfo {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionPrimitives {
    pub choice: bool,
    pub noul: bool,
    pub score: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DecisionQuestion {
    Choice { options: BTreeMap<String, String> },
    Noul { criteria: Option<NoulCriteria> },
    Score { levels: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoulCriteria {
    #[serde(rename = "true")]
    pub true_def: String,
    #[serde(rename = "false")]
    pub false_def: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub state: Value,
    pub instructions: String,
    pub question: DecisionQuestion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedDecisionQuestion {
    pub instructions: String,
    pub question: DecisionQuestion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionQuestionsRequest {
    pub state: Value,
    pub questions: BTreeMap<String, NamedDecisionQuestion>,
}

pub type DecisionAnswers = BTreeMap<String, DecisionAnswer>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionPricing {
    pub display_name: String,
    pub input_usd_per_million: f64,
    pub output_usd_per_million: f64,
}

impl DecisionPricing {
    pub fn estimated_usd(&self, usage: DecisionUsage) -> Option<f64> {
        if !self.input_usd_per_million.is_finite()
            || self.input_usd_per_million < 0.0
            || !self.output_usd_per_million.is_finite()
            || self.output_usd_per_million < 0.0
        {
            return None;
        }
        let value = (usage.input_tokens as f64 * self.input_usd_per_million
            + usage.output_tokens as f64 * self.output_usd_per_million)
            / 1_000_000.0;
        value.is_finite().then_some(value)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResponse {
    pub answers: DecisionAnswers,
    pub usage: Option<DecisionUsage>,
}

impl DecisionQuestionsRequest {
    pub fn validate_answers(&self, answers: &DecisionAnswers) -> Result<(), DecisionError> {
        if self.questions.is_empty() || answers.len() != self.questions.len() {
            return Err(DecisionError::Parse("invalid_answer_set".into()));
        }
        for (name, spec) in &self.questions {
            let answer = answers
                .get(name)
                .ok_or_else(|| DecisionError::Parse("missing_answer".into()))?;
            spec.question.validate_answer(answer)?;
        }
        Ok(())
    }
}

pub fn decide_questions_sequentially<M: DecisionModel + ?Sized>(
    model: &M,
    request: &DecisionQuestionsRequest,
    deadline: Instant,
) -> Result<DecisionResponse, DecisionError> {
    let mut answers = BTreeMap::new();
    for (name, spec) in &request.questions {
        if deadline::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        let single = DecisionRequest {
            state: request.state.clone(),
            instructions: spec.instructions.clone(),
            question: spec.question.clone(),
        };
        answers.insert(name.clone(), model.decide(&single, deadline)?);
    }
    if deadline::now() >= deadline {
        return Err(DecisionError::Deadline);
    }
    request.validate_answers(&answers)?;
    Ok(DecisionResponse {
        answers,
        usage: None,
    })
}

impl DecisionQuestion {
    pub fn validate_answer(&self, answer: &DecisionAnswer) -> Result<(), DecisionError> {
        let probability = |p: f64| p.is_finite() && (0.0..=1.0).contains(&p);
        let valid = match (self, answer) {
            (
                Self::Choice { options },
                DecisionAnswer::Choice {
                    choice,
                    probabilities,
                    confidence,
                },
            ) => {
                options.contains_key(choice)
                    && !probabilities.is_empty()
                    && probabilities.keys().all(|key| options.contains_key(key))
                    && probabilities.values().all(|p| probability(*p))
                    && confidence.is_none_or(probability)
            }
            (Self::Noul { .. }, DecisionAnswer::Noul { p }) => probability(*p),
            (
                Self::Score { levels },
                DecisionAnswer::Score {
                    score,
                    probabilities,
                    confidence,
                },
            ) => {
                let mut indexes = std::collections::BTreeSet::new();
                score.is_finite()
                    && !probabilities.is_empty()
                    && probabilities.iter().all(|(key, p)| {
                        key.parse::<usize>()
                            .ok()
                            .filter(|index| *index < levels.len() && indexes.insert(*index))
                            .is_some()
                            && probability(*p)
                    })
                    && confidence.is_none_or(probability)
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(DecisionError::Parse("invalid_answer".into()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DecisionAnswer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
    Noul {
        p: f64,
    },
    Score {
        score: f64,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionError {
    Unavailable(String),
    Unsupported,
    Deadline,
    Budget,
    CreditExhausted,
    Http(u16),
    Transport(String),
    Parse(String),
}

impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "decision model unavailable: {reason}"),
            Self::Unsupported => f.write_str("decision primitive unsupported"),
            Self::CreditExhausted => f.write_str("supporter credits exhausted"),
            Self::Budget => f.write_str("supporter call budget reached"),
            Self::Deadline => f.write_str("decision deadline exceeded"),
            Self::Http(status) => write!(f, "decision http status {status}"),
            Self::Transport(reason) => write!(f, "decision transport error: {reason}"),
            Self::Parse(reason) => write!(f, "decision parse error: {reason}"),
        }
    }
}

impl std::error::Error for DecisionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decision_request_and_answer_keep_the_plugin_wire_shape() {
        let request = DecisionRequest {
            state: json!({ "candidate": "a" }),
            instructions: "Choose one".into(),
            question: DecisionQuestion::Choice {
                options: BTreeMap::from([(String::from("a"), String::from("A"))]),
            },
        };
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({
                "state": { "candidate": "a" },
                "instructions": "Choose one",
                "question": { "type": "choice", "options": { "a": "A" } }
            })
        );
        let answer = DecisionAnswer::Noul { p: 0.75 };
        assert_eq!(
            serde_json::to_value(&answer).unwrap(),
            json!({ "type": "noul", "p": 0.75 })
        );
        assert_eq!(
            serde_json::from_value::<DecisionAnswer>(serde_json::to_value(answer).unwrap())
                .unwrap(),
            DecisionAnswer::Noul { p: 0.75 }
        );
    }

    #[test]
    fn noul_criteria_use_the_native_true_false_keys() {
        let question = DecisionQuestion::Noul {
            criteria: Some(NoulCriteria {
                true_def: "yes".into(),
                false_def: "no".into(),
            }),
        };
        assert_eq!(
            serde_json::to_value(question).unwrap(),
            json!({
                "type": "noul",
                "criteria": { "true": "yes", "false": "no" }
            })
        );
    }
}

#[cfg(test)]
mod questions_tests;
