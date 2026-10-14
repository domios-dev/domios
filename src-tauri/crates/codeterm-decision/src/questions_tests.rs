use super::*;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;

struct SequentialModel {
    seen: Mutex<Vec<DecisionRequest>>,
    invalid: bool,
}

impl DecisionModel for SequentialModel {
    fn id(&self) -> &str {
        "local-fixture"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            ..Default::default()
        }
    }
    fn decide(
        &self,
        request: &DecisionRequest,
        _: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        self.seen.lock().unwrap().push(request.clone());
        Ok(if self.invalid {
            DecisionAnswer::Noul { p: f64::NAN }
        } else {
            DecisionAnswer::Noul { p: 0.7 }
        })
    }
}

fn request() -> DecisionQuestionsRequest {
    DecisionQuestionsRequest {
        state: json!({ "fixture": "opaque shared state" }),
        questions: BTreeMap::from([
            (
                "a".into(),
                NamedDecisionQuestion {
                    instructions: "opaque-a".into(),
                    question: DecisionQuestion::Noul { criteria: None },
                },
            ),
            (
                "b".into(),
                NamedDecisionQuestion {
                    instructions: "opaque-b".into(),
                    question: DecisionQuestion::Noul { criteria: None },
                },
            ),
        ]),
    }
}

#[test]
fn decision_questions_round_trip_named_wire_and_answer_map() {
    let request = request();
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["state"], request.state);
    assert_eq!(encoded["questions"]["a"]["instructions"], "opaque-a");
    assert!(encoded.get("question").is_none());
    assert_eq!(
        serde_json::from_value::<DecisionQuestionsRequest>(encoded).unwrap(),
        request
    );
    let response = DecisionResponse {
        answers: BTreeMap::from([
            ("a".into(), DecisionAnswer::Noul { p: 0.7 }),
            ("b".into(), DecisionAnswer::Noul { p: 0.3 }),
        ]),
        usage: Some(DecisionUsage {
            input_tokens: 10,
            output_tokens: 2,
        }),
    };
    assert_eq!(
        serde_json::from_value::<DecisionResponse>(serde_json::to_value(&response).unwrap())
            .unwrap(),
        response
    );
}

#[test]
fn decision_questions_sequential_adapter_reuses_one_state_and_stays_all_or_nothing() {
    let request = request();
    for invalid in [false, true] {
        let model = SequentialModel {
            seen: Mutex::new(Vec::new()),
            invalid,
        };
        let response = model.decide_questions(&request, Instant::now() + Duration::from_secs(1));
        if invalid {
            assert!(matches!(response, Err(DecisionError::Parse(_))));
        } else {
            assert_eq!(response.unwrap().answers.len(), 2);
        }
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen.iter().all(|single| single.state == request.state));
        assert_eq!(seen[0].instructions, request.questions["a"].instructions);
        assert_eq!(seen[1].instructions, request.questions["b"].instructions);
    }
    let model = SequentialModel {
        seen: Mutex::new(Vec::new()),
        invalid: false,
    };
    assert_eq!(
        model.decide_questions(&request, Instant::now()),
        Err(DecisionError::Deadline)
    );
    assert!(model.seen.lock().unwrap().is_empty());
}
