use std::sync::Arc;
use std::time::Instant;

use codeterm_types::decision::{
    decide_questions_sequentially, DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo,
    DecisionPricing, DecisionPrimitives, DecisionQuestionsRequest, DecisionRequest,
    DecisionResponse, TokenMode,
};
use serde_json::json;

use super::vm::{call_json, PluginVm, VmError};

pub struct JsDecisionModel {
    vm: Arc<PluginVm>,
    id: String,
    token_mode: TokenMode,
}

impl JsDecisionModel {
    pub fn new(vm: Arc<PluginVm>, id: impl Into<String>, token_mode: TokenMode) -> Self {
        Self {
            vm,
            id: id.into(),
            token_mode,
        }
    }
}

impl JsDecisionModel {
    fn exec_env_scope(&self) -> Result<super::host_exec::HostExecEnvGuard, DecisionError> {
        let env = self
            .vm
            .service()
            .map(|host| host.model_provider_process_env(&self.id))
            .transpose()
            .map_err(|_| DecisionError::Unavailable("provider_credentials".into()))?
            .unwrap_or_default();
        Ok(super::host_exec::host_env_scope(env))
    }
}

impl DecisionModel for JsDecisionModel {
    fn supports_calibrated_choice_confidence(&self) -> bool {
        call_json_or_default(&self.vm, &self.id, "supportsCalibratedChoiceConfidence")
    }
    fn supports_question_batch(&self) -> bool {
        super::vm::has_method(&self.vm, "decideQuestions")
            && call_json_or_default::<bool>(&self.vm, &self.id, "supportsQuestionBatch")
    }
    fn id(&self) -> &str {
        &self.id
    }

    fn model_id(&self) -> Option<String> {
        call_json_or_default(&self.vm, &self.id, "modelId")
    }

    fn pricing(&self) -> Option<DecisionPricing> {
        call_json_or_default(&self.vm, &self.id, "pricing")
    }

    fn metadata(&self) -> codeterm_types::decision::DecisionMetadata {
        call_json(&self.vm, "metadata", &[]).unwrap_or_default()
    }

    fn select_model(&self, id: &str) -> bool {
        let Ok(_env) = self.exec_env_scope() else {
            return false;
        };
        if !self.models().iter().any(|model| model.id == id) {
            return false;
        }
        match call_json::<bool>(&self.vm, "selectModel", &[json!(id)]) {
            Ok(selected) => selected,
            Err(VmError::MissingFunction(_)) => self.model_id().as_deref() == Some(id),
            Err(_) => false,
        }
    }

    fn token_mode(&self) -> TokenMode {
        self.token_mode
    }

    fn primitives(&self) -> DecisionPrimitives {
        call_json_or_default(&self.vm, &self.id, "primitives")
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        let Ok(_env) = self.exec_env_scope() else {
            return Vec::new();
        };
        call_json_or_default(&self.vm, &self.id, "models")
    }

    fn decide_questions(
        &self,
        request: &DecisionQuestionsRequest,
        deadline: Instant,
    ) -> Result<DecisionResponse, DecisionError> {
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        let encoded = serde_json::to_value(request)
            .map_err(|_| DecisionError::Parse("invalid_request".into()))?;
        let _env = self.exec_env_scope()?;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        let response: DecisionResponse = match call_json::<serde_json::Value>(
            &self.vm,
            "decideQuestions",
            &[encoded, json!(remaining)],
        ) {
            Ok(answers) => decode_result(answers)?,
            Err(VmError::MissingFunction(_)) => {
                return decide_questions_sequentially(self, request, deadline)
            }
            Err(error) => return Err(map_vm_error(error)),
        };
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        request.validate_answers(&response.answers)?;
        Ok(response)
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
        let _env = self.exec_env_scope()?;
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        let answer: DecisionAnswer = decode_result(
            call_json(&self.vm, "decide", &[encoded, json!(remaining)]).map_err(map_vm_error)?,
        )?;
        if Instant::now() >= deadline {
            return Err(DecisionError::Deadline);
        }
        request.question.validate_answer(&answer)?;
        Ok(answer)
    }
}

fn decode_result<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, DecisionError> {
    if let Some(error) = value.get("error").and_then(serde_json::Value::as_str) {
        return Err(match error {
            "unavailable" => DecisionError::Unavailable("plugin_unavailable".into()),
            "timeout" => DecisionError::Deadline,
            "transport" => DecisionError::Transport("plugin_decide_failed".into()),
            _ => DecisionError::Parse("invalid_answer".into()),
        });
    }
    serde_json::from_value(value).map_err(|_| DecisionError::Parse("invalid_answer".into()))
}

fn call_json_or_default<T>(vm: &PluginVm, model_id: &str, method: &str) -> T
where
    T: serde::de::DeserializeOwned + Default,
{
    call_json(vm, method, &[]).unwrap_or_else(|_| {
        log::debug!("decision model {model_id}.{method} unavailable");
        T::default()
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_types::decision::DecisionQuestion;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn decision_exec_scope_restores_prior_env_without_cross_adapter_leak() {
        let original = super::super::host_exec::current_host_env();
        let prior = std::collections::HashMap::from([("OUTER_SCOPE".into(), "opaque".into())]);
        let _outer = super::super::host_exec::host_env_scope(prior.clone());
        let model = JsDecisionModel::new(
            vm("module.exports.default = {};"),
            "fixture",
            TokenMode::None,
        );
        {
            let _scope = model.exec_env_scope().unwrap();
            assert!(super::super::host_exec::current_host_env().is_empty());
        }
        assert_eq!(super::super::host_exec::current_host_env(), prior);
        drop(_outer);
        assert_eq!(super::super::host_exec::current_host_env(), original);
    }

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
        let vm = vm(r#"module.exports.default = {
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
            }"#);
        let model = JsDecisionModel::new(vm, "decision-fixture", TokenMode::Optional);
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
                .decide(
                    &request(DecisionQuestion::Noul { criteria: None }),
                    deadline()
                )
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
            vm(r#"module.exports.default = {
                    models() { return [{ id: "loaded", display_name: "Loaded model" }]; },
                    selectModel(id) { return id === "loaded"; }
                };"#),
            "decision-models",
            TokenMode::Optional,
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
            TokenMode::Optional,
        );
        let request = request(DecisionQuestion::Noul { criteria: None });
        assert!(matches!(
            throwing.decide(&request, deadline()),
            Err(DecisionError::Transport(reason)) if reason == "plugin_decide_failed"
        ));

        let malformed = JsDecisionModel::new(
            vm(r#"module.exports.default = { decide() { return { type: "noul", p: "bad" }; } };"#),
            "decision-malformed",
            TokenMode::Optional,
        );
        assert!(matches!(
            malformed.decide(&request, deadline()),
            Err(DecisionError::Parse(reason)) if reason == "invalid_answer"
        ));
    }
}

#[cfg(test)]
mod questions_tests {
    use super::*;
    use codeterm_types::decision::{DecisionQuestion, DecisionUsage, NamedDecisionQuestion};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn js_decision_model_shared_request_preserves_usage_and_rejects_missing_answers() {
        let request = DecisionQuestionsRequest {
            state: json!({ "screen": "opaque" }),
            questions: BTreeMap::from([
                (
                    "first".into(),
                    NamedDecisionQuestion {
                        instructions: "a".into(),
                        question: DecisionQuestion::Noul { criteria: None },
                    },
                ),
                (
                    "second".into(),
                    NamedDecisionQuestion {
                        instructions: "b".into(),
                        question: DecisionQuestion::Noul { criteria: None },
                    },
                ),
            ]),
        };
        for missing in [false, true] {
            let source = format!(
                r#"module.exports.default = {{
                decide() {{ throw new Error("one shared call required"); }},
                decideQuestions(request) {{
                    if (request.state.screen !== "opaque") throw new Error("wrong state");
                    var answers = {{ first: {{ type: "noul", p: 0.9 }} }};
                    if (!{missing}) answers.second = {{ type: "noul", p: 0.8 }};
                    return {{ answers: answers, usage: {{ input_tokens: 123, output_tokens: 45 }} }};
                }}
            }};"#
            );
            let model = JsDecisionModel::new(
                Arc::new(PluginVm::from_source("fixture", source).unwrap()),
                "fixture",
                TokenMode::None,
            );
            let result = model.decide_questions(&request, Instant::now() + Duration::from_secs(2));
            if missing {
                assert!(matches!(result, Err(DecisionError::Parse(_))));
            } else {
                assert_eq!(
                    result.unwrap().usage,
                    Some(DecisionUsage {
                        input_tokens: 123,
                        output_tokens: 45
                    })
                );
            }
        }
    }
}
