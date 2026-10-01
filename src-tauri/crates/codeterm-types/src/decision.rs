use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub trait DecisionModel: Send + Sync {
    fn id(&self) -> &str;
    fn primitives(&self) -> DecisionPrimitives;
    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError>;
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
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DecisionAnswer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: Option<f64>,
    },
    Noul { p: f64 },
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
    Http(u16),
    Transport(String),
    Parse(String),
}

impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(reason) => write!(f, "decision model unavailable: {reason}"),
            Self::Unsupported => f.write_str("decision primitive unsupported"),
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
