use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    DecisionQuestion, DecisionRequest,
};
use serde_json::json;

use super::vm::{call_json, PluginVm, VmError};

pub struct JsDecisionModel {
    vm: Arc<PluginVm>,
    id: String,
}

impl JsDecisionModel {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>) -> Self {
        Self { vm, id: id.into() }
    }
}

impl DecisionModel for JsDecisionModel {
    fn id(&self) -> &str {
        &self.id
    }

    fn primitives(&self) -> DecisionPrimitives {
        call_json(&self.vm, "primitives", &[]).unwrap_or_default()
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        call_json(&self.vm, "models", &[]).unwrap_or_default()
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        let encoded = serde_json::to_value(request)
            .map_err(|_| DecisionError::Parse("invalid_request".into()))?;
        let answer: DecisionAnswer = call_json(&self.vm, "decide", &[json!(encoded)])
            .map_err(map_vm_error)?;
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        validate_answer(request, &answer)?;
        Ok(answer)
    }
}

fn map_vm_error(error: VmError) -> DecisionError {
    match error {
        VmError::InvalidJson(..) | VmError::ArgSerialize(..) => {
            DecisionError::Parse("invalid_answer".into())
        }
        VmError::AsyncJobTimedOut => DecisionError::Deadline,
        VmError::MissingFunction(_) => DecisionError::Unsupported,
        _ => DecisionError::Transport("plugin_decide_failed".into()),
    }
}

fn valid_probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn valid_confidence(value: Option<f64>) -> bool {
    value.is_none_or(valid_probability)
}

fn validate_answer(
    request: &DecisionRequest,
    answer: &DecisionAnswer,
) -> Result<(), DecisionError> {
    let valid = match (&request.question, answer) {
        (
            DecisionQuestion::Choice { options },
            DecisionAnswer::Choice {
                choice,
                probabilities,
                confidence,
            },
        ) => {
            options.contains_key(choice)
                && !probabilities.is_empty()
                && probabilities.keys().all(|key| options.contains_key(key))
                && probabilities.values().all(|p| valid_probability(*p))
                && valid_confidence(*confidence)
        }
        (DecisionQuestion::Noul { .. }, DecisionAnswer::Noul { p }) => valid_probability(*p),
        (
            DecisionQuestion::Score { levels },
            DecisionAnswer::Score {
                score,
                probabilities,
                confidence,
            },
        ) => {
            let mut indexes = BTreeSet::new();
            score.is_finite()
                && !probabilities.is_empty()
                && probabilities.iter().all(|(key, value)| {
                    key.parse::<usize>()
                        .ok()
                        .filter(|index| *index < levels.len() && indexes.insert(*index))
                        .is_some()
                        && valid_probability(*value)
                })
                && valid_confidence(*confidence)
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(DecisionError::Parse("invalid_answer".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn vm(source: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("decision-fixture", source.into()).expect("vm"))
    }

    fn request(question: DecisionQuestion) -> DecisionRequest {
        DecisionRequest {
            state: json!({ "candidate": "fixture" }),
            instructions: "Answer the fixture".into(),
            question,
        }
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }

    #[test]
    fn js_decision_model_decides_noul_choice_score() {
        let vm = vm(
            r#"module.exports.default = {
                primitives() { return { choice: true, noul: true, score: true }; },
                decide(request) {
                    if (request.question.type === "noul") return { type: "noul", p: 0.8 };
                    if (request.question.type === "choice") return {
                        type: "choice", choice: "yes", probabilities: { yes: 0.8, no: 0.2 }, confidence: 0.8
                    };
                    return {
                        type: "score", score: 2, probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 }, confidence: 0.7
                    };
                }
            }"#,
        );
        let model = JsDecisionModel::new(vm, "decision-fixture");
        assert_eq!(
            model.primitives(),
            DecisionPrimitives {
                choice: true,
                noul: true,
                score: true,
            }
        );
        assert_eq!(
            model
                .decide(&request(DecisionQuestion::Noul { criteria: None }), deadline())
                .unwrap(),
            DecisionAnswer::Noul { p: 0.8 }
        );
        assert_eq!(
            model
                .decide(
                    &request(DecisionQuestion::Choice {
                        options: BTreeMap::from([
                            ("yes".into(), "Yes".into()),
                            ("no".into(), "No".into()),
                        ]),
                    }),
                    deadline(),
                )
                .unwrap(),
            DecisionAnswer::Choice {
                choice: "yes".into(),
                probabilities: BTreeMap::from([("no".into(), 0.2), ("yes".into(), 0.8)]),
                confidence: Some(0.8),
            }
        );
        assert_eq!(
            model
                .decide(
                    &request(DecisionQuestion::Score {
                        levels: vec!["low".into(), "middle".into(), "high".into()],
                    }),
                    deadline(),
                )
                .unwrap(),
            DecisionAnswer::Score {
                score: 2.0,
                probabilities: BTreeMap::from([
                    ("0".into(), 0.1),
                    ("1".into(), 0.2),
                    ("2".into(), 0.7),
                ]),
                confidence: Some(0.7),
            }
        );
    }

    #[test]
    fn js_decision_model_models() {
        let model = JsDecisionModel::new(
            vm(
                r#"module.exports.default = {
                    models() { return [{ id: "loaded", display_name: "Loaded model" }]; }
                };"#,
            ),
            "decision-models",
        );
        assert_eq!(
            model.models(),
            vec![DecisionModelInfo {
                id: "loaded".into(),
                display_name: "Loaded model".into(),
            }]
        );
        assert!(model.select_model("loaded"));
        assert!(!model.select_model("missing"));
    }

    #[test]
    fn js_decision_model_error_maps_without_panic() {
        let throwing = JsDecisionModel::new(
            vm(r#"module.exports.default = { decide() { throw new Error("private detail"); } };"#),
            "decision-throw",
        );
        let request = request(DecisionQuestion::Noul { criteria: None });
        assert!(matches!(
            throwing.decide(&request, deadline()),
            Err(DecisionError::Transport(reason)) if reason == "plugin_decide_failed"
        ));

        let malformed = JsDecisionModel::new(
            vm(r#"module.exports.default = { decide() { return { type: "noul", p: "bad" }; } };"#),
            "decision-malformed",
        );
        assert!(matches!(
            malformed.decide(&request, deadline()),
            Err(DecisionError::Parse(reason)) if reason == "invalid_answer"
        ));
    }
}
