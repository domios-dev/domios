use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::time::Instant;

use serde_json::{json, Value};
use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    DecisionQuestion, DecisionRequest,
};

use super::token_store::TokenStore;

const DEFAULT_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const DEFAULT_MODEL: &str = "jev-latest";
static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

pub struct TypeSafeAdapter {
    token_store: TokenStore,
    endpoint: String,
    model: RwLock<String>,
}

impl fmt::Debug for TypeSafeAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypeSafeAdapter")
            .field("id", &Self::ID)
            .finish()
    }
}

impl TypeSafeAdapter {
    pub const ID: &str = "typesafe-jev";

    pub fn new(token_store: TokenStore, endpoint: Option<String>, model: Option<String>) -> Self {
        Self {
            token_store,
            endpoint: endpoint.unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned()),
            model: RwLock::new(model.unwrap_or_else(|| DEFAULT_MODEL.to_owned())),
        }
    }

    pub fn build_request_body(model: &str, request: &DecisionRequest) -> Value {
        let question = match &request.question {
            DecisionQuestion::Choice { options } => {
                json!({"type": "choice", "instructions": request.instructions, "criteria": options})
            }
            DecisionQuestion::Noul { criteria } => {
                let mut question = json!({"type": "noul", "instructions": request.instructions});
                if let Some(criteria) = criteria {
                    question["criteria"] = json!({
                        "true": criteria.true_def,
                        "false": criteria.false_def,
                    });
                }
                question
            }
            DecisionQuestion::Score { levels } => {
                json!({"type": "score", "instructions": request.instructions, "criteria": levels})
            }
        };
        json!({"state": request.state, "model": model, "questions": {"q": question}})
    }

    pub fn parse_answer(
        question: &DecisionQuestion,
        response: &Value,
    ) -> Result<DecisionAnswer, DecisionError> {
        let answer = response.pointer("/answers/q").ok_or_else(parse_error)?;
        let kind = answer
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(parse_error)?;
        match question {
            DecisionQuestion::Choice { options } if kind == "choice" => {
                let choice = answer
                    .get("choice")
                    .and_then(Value::as_str)
                    .ok_or_else(parse_error)?;
                if !options.contains_key(choice) {
                    return Err(parse_error());
                }
                let probabilities = probabilities(answer)?;
                if probabilities.is_empty()
                    || !probabilities.keys().all(|key| options.contains_key(key))
                {
                    return Err(parse_error());
                }
                Ok(DecisionAnswer::Choice {
                    choice: choice.to_owned(),
                    probabilities,
                    confidence: optional_number(answer, "confidence")?,
                })
            }
            DecisionQuestion::Score { levels } if kind == "score" => {
                let score = answer
                    .get("score")
                    .and_then(Value::as_f64)
                    .filter(|score| score.is_finite())
                    .ok_or_else(parse_error)?;
                let probabilities = probabilities(answer)?;
                if probabilities.is_empty() {
                    return Err(parse_error());
                }
                let mut indexed = BTreeMap::new();
                for (key, probability) in probabilities {
                    let index = key.parse::<usize>().map_err(|_| parse_error())?;
                    if index >= levels.len()
                        || indexed.insert(index.to_string(), probability).is_some()
                    {
                        return Err(parse_error());
                    }
                }
                Ok(DecisionAnswer::Score {
                    score,
                    probabilities: indexed,
                    confidence: optional_number(answer, "confidence")?,
                })
            }
            DecisionQuestion::Noul { .. } if kind == "noul" => {
                let p = number(answer, "noul")?;
                Ok(DecisionAnswer::Noul { p })
            }
            _ => Err(parse_error()),
        }
    }
}

fn parse_error() -> DecisionError {
    DecisionError::Parse("invalid_answer".to_owned())
}

fn number(value: &Value, key: &str) -> Result<f64, DecisionError> {
    let number = value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(parse_error)?;
    if number.is_finite() && (0.0..=1.0).contains(&number) {
        Ok(number)
    } else {
        Err(parse_error())
    }
}

fn optional_number(value: &Value, key: &str) -> Result<Option<f64>, DecisionError> {
    value.get(key).map(|_| number(value, key)).transpose()
}

fn probabilities(answer: &Value) -> Result<BTreeMap<String, f64>, DecisionError> {
    let object = answer
        .get("probabilities")
        .and_then(Value::as_object)
        .ok_or_else(parse_error)?;
    object
        .iter()
        .map(|(key, value)| {
            let p = value
                .as_f64()
                .filter(|p| p.is_finite() && (0.0..=1.0).contains(p))
                .ok_or_else(parse_error)?;
            Ok((key.clone(), p))
        })
        .collect()
}

impl DecisionModel for TypeSafeAdapter {
    fn id(&self) -> &str {
        Self::ID
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: true,
            score: true,
        }
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        let model = self
            .model
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        vec![DecisionModelInfo {
            id: model.clone(),
            display_name: model,
        }]
    }

    fn select_model(&self, id: &str) -> bool {
        let model = self
            .model
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        model.as_str() == id
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        let token = self
            .token_store
            .load(Self::ID)
            .map_err(|_| DecisionError::Unavailable("token_store_error".to_owned()))?
            .ok_or_else(|| DecisionError::Unavailable("token_missing".to_owned()))?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(DecisionError::Deadline);
        }
        let client = CLIENT.get_or_init(reqwest::blocking::Client::new);
        let model = self
            .model
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let body = Self::build_request_body(&model, request);
        let response = client
            .post(&self.endpoint)
            .bearer_auth(token.expose())
            .timeout(remaining)
            .json(&body)
            .send()
            .map_err(|error| {
                if error.is_timeout() {
                    DecisionError::Deadline
                } else {
                    DecisionError::Transport("request_failed".to_owned())
                }
            })?;
        if !response.status().is_success() {
            return Err(DecisionError::Http(response.status().as_u16()));
        }
        let body: Value = response
            .json()
            .map_err(|_| DecisionError::Parse("invalid_json".to_owned()))?;
        Self::parse_answer(&request.question, &body)
    }
}

#[cfg(test)]
mod tests;
