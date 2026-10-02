use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPricing,
    DecisionPrimitives, DecisionQuestion, DecisionQuestionsRequest, DecisionRequest,
    NamedDecisionQuestion, TokenMode,
};
use serde_json::json;

use crate::js_runtime::{JsDecisionModel, PluginVm};

struct NativeDecisionModel(TokenMode);

impl DecisionModel for NativeDecisionModel {
    fn id(&self) -> &str {
        "native-fixture"
    }

    fn model_id(&self) -> Option<String> {
        Some("fixture-model".into())
    }

    fn pricing(&self) -> Option<DecisionPricing> {
        Some(DecisionPricing {
            display_name: "local fixture".into(),
            input_usd_per_million: 0.0,
            output_usd_per_million: 0.0,
        })
    }

    fn token_mode(&self) -> TokenMode {
        self.0
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: true,
            score: true,
        }
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        vec![DecisionModelInfo {
            id: "fixture-model".into(),
            display_name: "Fixture model".into(),
        }]
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        _deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        Ok(match &request.question {
            DecisionQuestion::Noul { .. } => DecisionAnswer::Noul { p: 0.8 },
            DecisionQuestion::Choice { .. } => DecisionAnswer::Choice {
                choice: "yes".into(),
                probabilities: BTreeMap::from([("no".into(), 0.2), ("yes".into(), 0.8)]),
                confidence: Some(0.8),
            },
            DecisionQuestion::Score { .. } => DecisionAnswer::Score {
                score: 2.0,
                probabilities: BTreeMap::from([
                    ("0".into(), 0.1),
                    ("1".into(), 0.2),
                    ("2".into(), 0.7),
                ]),
                confidence: Some(0.7),
            },
        })
    }
}

fn shared_fixtures() -> Vec<DecisionRequest> {
    vec![
        DecisionRequest {
            state: json!({ "fixture": "noul" }),
            instructions: "Noul fixture".into(),
            question: DecisionQuestion::Noul { criteria: None },
        },
        DecisionRequest {
            state: json!({ "fixture": "choice" }),
            instructions: "Choice fixture".into(),
            question: DecisionQuestion::Choice {
                options: BTreeMap::from([("no".into(), "No".into()), ("yes".into(), "Yes".into())]),
            },
        },
        DecisionRequest {
            state: json!({ "fixture": "score" }),
            instructions: "Score fixture".into(),
            question: DecisionQuestion::Score {
                levels: vec!["low".into(), "medium".into(), "high".into()],
            },
        },
    ]
}

#[test]
fn parity_decision_model_matches_shared_fixtures() {
    let source = r#"module.exports.default = {
        modelId() { return "fixture-model"; },
        pricing() { return { display_name: "local fixture", input_usd_per_million: 0, output_usd_per_million: 0 }; },
        primitives() { return { choice: true, noul: true, score: true }; },
        models() { return [{ id: "fixture-model", display_name: "Fixture model" }]; },
        decide(request) {
            if (request.question.type === "noul") return { type: "noul", p: 0.8 };
            if (request.question.type === "choice") return {
                type: "choice", choice: "yes", probabilities: { no: 0.2, yes: 0.8 }, confidence: 0.8
            };
            return {
                type: "score", score: 2, probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 }, confidence: 0.7
            };
        }
    };"#;
    let vm = Arc::new(PluginVm::from_source("js-fixture", source.into()).expect("vm"));
    let forms = [
        (json!(true), TokenMode::Optional),
        (json!({ "tokenMode": "required" }), TokenMode::Required),
        (json!({ "tokenMode": "none" }), TokenMode::None),
    ];
    for (form, expected) in forms {
        let caps: crate::js_runtime::manifest::Capabilities =
            serde_json::from_value(json!({ "decisionModel": form })).expect("manifest cap");
        let native = NativeDecisionModel(expected);
        let javascript =
            JsDecisionModel::new(vm.clone(), "js-fixture", caps.decision_model.token_mode);
        assert_eq!(native.token_mode(), expected);
        assert_eq!(native.token_mode(), javascript.token_mode());
        assert_eq!(native.primitives(), javascript.primitives());
        assert_eq!(native.models(), javascript.models());
        assert_eq!(native.model_id(), javascript.model_id());
        assert_eq!(native.pricing(), javascript.pricing());
        assert!(native.select_model("fixture-model"));
        assert_eq!(
            native.select_model("fixture-model"),
            javascript.select_model("fixture-model")
        );
        assert_eq!(
            native.select_model("missing"),
            javascript.select_model("missing")
        );
        for request in shared_fixtures() {
            let deadline = Instant::now() + std::time::Duration::from_secs(2);
            assert_eq!(
                native.decide(&request, deadline),
                javascript.decide(&request, deadline),
            );
        }
    }
}

#[test]
fn parity_decision_model_multi_question_shared_state_and_sequential_local_adapter() {
    let decide = r#"decide(request) {
        if (JSON.stringify(request.state) !== JSON.stringify({ fixture: "shared" })) throw new Error("state mismatch");
        if (request.question.type === "noul") return { type: "noul", p: 0.8 };
        if (request.question.type === "choice") return { type: "choice", choice: "yes", probabilities: { no: 0.2, yes: 0.8 }, confidence: 0.8 };
        return { type: "score", score: 2, probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 }, confidence: 0.7 };
    }"#;
    let multi = r#"decideQuestions(request) {
        var answers = {};
        for (var name of Object.keys(request.questions)) {
            var spec = request.questions[name];
            answers[name] = this.decide({ state: request.state, instructions: spec.instructions, question: spec.question });
        }
        return { answers: answers, usage: null };
    }"#;
    let request = DecisionQuestionsRequest {
        state: json!({ "fixture": "shared" }),
        questions: shared_fixtures()
            .into_iter()
            .enumerate()
            .map(|(index, single)| {
                (
                    format!("question_{index}"),
                    NamedDecisionQuestion {
                        instructions: single.instructions,
                        question: single.question,
                    },
                )
            })
            .collect(),
    };
    for extension in ["", multi] {
        let source = format!("module.exports.default = {{ {decide}, {extension} }};");
        let vm = Arc::new(PluginVm::from_source("shared-fixture", source).unwrap());
        let js = JsDecisionModel::new(vm, "shared-fixture", TokenMode::None);
        let native = NativeDecisionModel(TokenMode::None);
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        let response = js.decide_questions(&request, deadline).unwrap();
        assert_eq!(
            response,
            native.decide_questions(&request, deadline).unwrap()
        );
        assert_eq!(response.answers.len(), 3);
    }
}
