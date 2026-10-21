use std::collections::BTreeMap;
use std::fmt;
use std::sync::RwLock;
use std::time::Instant;

use codeterm_decision::{
    DecisionAnswer, DecisionAnswers, DecisionError, DecisionModel, DecisionModelInfo,
    DecisionPricing, DecisionPrimitives, DecisionQuestion, DecisionQuestionsRequest,
    DecisionRequest, DecisionResponse, DecisionUsage, NamedDecisionQuestion, TokenMode,
};
use serde_json::{json, Value};

use super::token_store::TokenStore;

const DEFAULT_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const DEFAULT_MODEL: &str = "jev-latest";

pub struct TypeSafeAdapter {
    token_store: TokenStore,
    endpoint: String,
    client: Result<reqwest::blocking::Client, DecisionError>,
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
            client: reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .build()
                .map_err(|_| DecisionError::Unavailable("http_client_unavailable".into())),
            model: RwLock::new(
                model
                    .filter(|id| id.starts_with("jev-"))
                    .unwrap_or_else(|| DEFAULT_MODEL.to_owned()),
            ),
        }
    }

    pub fn build_request_body(model: &str, request: &DecisionRequest) -> Value {
        Self::build_questions_body(
            model,
            &DecisionQuestionsRequest {
                state: request.state.clone(),
                questions: BTreeMap::from([(
                    "q".into(),
                    NamedDecisionQuestion {
                        instructions: request.instructions.clone(),
                        question: request.question.clone(),
                    },
                )]),
            },
        )
    }

    pub fn build_questions_body(model: &str, request: &DecisionQuestionsRequest) -> Value {
        let questions: BTreeMap<_, _> = request
            .questions
            .iter()
            .map(|(name, spec)| (name.clone(), Self::question_body(spec)))
            .collect();
        json!({ "state": request.state, "model": model, "questions": questions })
    }

    fn question_body(spec: &NamedDecisionQuestion) -> Value {
        match &spec.question {
            DecisionQuestion::Choice { options } => {
                json!({ "type": "choice", "instructions": spec.instructions, "criteria": options })
            }
            DecisionQuestion::Noul { criteria } => {
                let mut question = json!({ "type": "noul", "instructions": spec.instructions });
                if let Some(criteria) = criteria {
                    question["criteria"] =
                        json!({ "true": criteria.true_def, "false": criteria.false_def });
                }
                question
            }
            DecisionQuestion::Score { levels } => {
                json!({ "type": "score", "instructions": spec.instructions, "criteria": levels })
            }
        }
    }

    pub fn parse_answers(
        request: &DecisionQuestionsRequest,
        response: &Value,
    ) -> Result<DecisionAnswers, DecisionError> {
        let values = response
            .get("answers")
            .and_then(Value::as_object)
            .ok_or_else(parse_error)?;
        if values.len() != request.questions.len() {
            return Err(parse_error());
        }
        let answers = request
            .questions
            .iter()
            .map(|(name, spec)| {
                let value = values.get(name).ok_or_else(parse_error)?;
                Ok((
                    name.clone(),
                    Self::parse_answer_value(&spec.question, value)?,
                ))
            })
            .collect::<Result<DecisionAnswers, DecisionError>>()?;
        request.validate_answers(&answers)?;
        Ok(answers)
    }

    pub fn parse_usage(response: &Value) -> Result<Option<DecisionUsage>, DecisionError> {
        response
            .get("usage")
            .map(|usage| serde_json::from_value(usage.clone()).map_err(|_| parse_error()))
            .transpose()
    }

    pub fn parse_answer(
        question: &DecisionQuestion,
        response: &Value,
    ) -> Result<DecisionAnswer, DecisionError> {
        let answer = response.pointer("/answers/q").ok_or_else(parse_error)?;
        Self::parse_answer_value(question, answer)
    }

    fn parse_answer_value(
        question: &DecisionQuestion,
        answer: &Value,
    ) -> Result<DecisionAnswer, DecisionError> {
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
    fn supports_calibrated_choice_confidence(&self) -> bool {
        true
    }
    fn supports_question_batch(&self) -> bool {
        true
    }
    fn id(&self) -> &str {
        Self::ID
    }

    fn model_id(&self) -> Option<String> {
        Some(
            self.model
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        )
    }

    fn pricing(&self) -> Option<DecisionPricing> {
        Some(DecisionPricing {
            display_name: "Jev".into(),
            input_usd_per_million: 0.042,
            output_usd_per_million: 0.0,
        })
    }

    fn metadata(&self) -> codeterm_decision::DecisionMetadata {
        codeterm_decision::DecisionMetadata {
            display_name: Some("TypeSafe Jev".into()),
            help_url: Some("https://docs.typesafe.ai".into()),
            ..Default::default()
        }
    }

    fn token_mode(&self) -> TokenMode {
        TokenMode::Required
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
        let mut ids = vec![DEFAULT_MODEL.to_owned()];
        if model != DEFAULT_MODEL {
            ids.push(model);
        }
        ids.into_iter()
            .map(|id| DecisionModelInfo {
                display_name: id.clone(),
                id,
            })
            .collect()
    }

    // TypeSafe serves the named jev-* versions; another adapter's id never applies here.
    fn select_model(&self, id: &str) -> bool {
        if !id.starts_with("jev-") {
            return false;
        }
        *self
            .model
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = id.to_owned();
        true
    }

    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        deadline: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        let body = self.send_request(request, deadline)?;
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        Ok(DecisionResponse {
            answers: Self::parse_answers(request, &body)?,
            usage: Self::parse_usage(&body)?,
        })
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        let body = self.send_request(
            &DecisionQuestionsRequest {
                state: request.state.clone(),
                questions: BTreeMap::from([(
                    "q".into(),
                    NamedDecisionQuestion {
                        instructions: request.instructions.clone(),
                        question: request.question.clone(),
                    },
                )]),
            },
            deadline,
        )?;
        Self::parse_answer(&request.question, &body)
    }
}

impl TypeSafeAdapter {
    fn send_request(
        &self,
        request: &DecisionQuestionsRequest,
        deadline: Instant,
    ) -> Result<Value, DecisionError> {
        let token = self
            .token_store
            .load(Self::ID)
            .map_err(|_| DecisionError::Unavailable("token_store_error".to_owned()))?
            .ok_or_else(|| DecisionError::Unavailable("token_missing".to_owned()))?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(DecisionError::Deadline);
        }
        let client = self.client.as_ref().map_err(Clone::clone)?;
        let model = self
            .model
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let body = Self::build_questions_body(&model, request);
        let request = client
            .post(&self.endpoint)
            .bearer_auth(token.expose())
            .timeout(remaining)
            .json(&body)
            .build()
            .map_err(|_| DecisionError::Transport("invalid_request".into()))?;
        codeterm_decision::physical::before_request()?;
        let response = client.execute(request).map_err(|error| {
            if error.is_timeout() {
                DecisionError::Deadline
            } else {
                DecisionError::Transport("request_failed".to_owned())
            }
        })?;
        let status = response.status();
        if status.as_u16() == 402 {
            return Err(DecisionError::CreditExhausted);
        }
        let body: Value = response.json().map_err(|_| {
            if status.is_success() {
                DecisionError::Parse("invalid_json".to_owned())
            } else {
                DecisionError::Http(status.as_u16())
            }
        })?;
        if matches!(
            body.pointer("/error/code").and_then(Value::as_str),
            Some("insufficient_credits" | "out_of_credits" | "credit_exhausted")
        ) {
            return Err(DecisionError::CreditExhausted);
        }
        if !status.is_success() {
            return Err(DecisionError::Http(status.as_u16()));
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests;
