use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use codeterm_types::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionQuestion,
    DecisionRequest,
};
use serde_json::json;

use crate::js_runtime::{JsDecisionModel, PluginVm};

struct NativeDecisionModel;

impl DecisionModel for NativeDecisionModel {
    fn id(&self) -> &str {
        "native-fixture"
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            noul: true,
            score: true,
        }
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
                options: BTreeMap::from([
                    ("no".into(), "No".into()),
                    ("yes".into(), "Yes".into()),
                ]),
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
        primitives() { return { choice: true, noul: true, score: true }; },
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
    let native = NativeDecisionModel;
    let javascript = JsDecisionModel::new(vm, "js-fixture");
    assert_eq!(native.primitives(), javascript.primitives());
    for request in shared_fixtures() {
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        assert_eq!(
            native.decide(&request, deadline),
            javascript.decide(&request, deadline),
        );
    }
}
