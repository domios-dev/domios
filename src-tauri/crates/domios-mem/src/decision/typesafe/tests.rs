use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::*;
use codeterm_decision::NoulCriteria;

const TOKEN: &str = "local-test-secret";
const ANSWER_NOUL: &str = include_str!("../fixtures/answer_noul.json");
const ANSWER_SCORE: &str = include_str!("../fixtures/answer_score.json");
const ANSWER_CHOICE: &str = include_str!("../fixtures/answer_choice.json");
const ANSWER_MALFORMED: &str = include_str!("../fixtures/answer_malformed.json");
const REQUEST_NOUL: &str = include_str!("../fixtures/request_noul.json");
const REQUEST_NOUL_PLAIN: &str = include_str!("../fixtures/request_noul_plain.json");
const REQUEST_SCORE: &str = include_str!("../fixtures/request_score.json");
const REQUEST_CHOICE: &str = include_str!("../fixtures/request_choice.json");

fn question(kind: &str) -> DecisionQuestion {
    match kind {
        "noul" => DecisionQuestion::Noul { criteria: None },
        "score" => DecisionQuestion::Score {
            levels: vec!["low".into(), "medium".into(), "high".into()],
        },
        "choice" => DecisionQuestion::Choice {
            options: BTreeMap::from([
                ("billing".into(), "Billing requests".into()),
                ("support".into(), "Technical requests".into()),
            ]),
        },
        _ => unreachable!(),
    }
}

fn request(kind: &str) -> DecisionRequest {
    DecisionRequest {
        state: Value::String("test-state".into()),
        instructions: "test-instructions".into(),
        question: question(kind),
    }
}

fn fixture(source: &str) -> Value {
    serde_json::from_str(source).unwrap()
}

#[test]
fn typesafe_jev_declares_a_required_token() {
    let adapter = TypeSafeAdapter::new(
        TokenStore::new(std::env::temp_dir().join("typesafe-token-mode-fixture")),
        None,
        None,
    );
    assert_eq!(adapter.token_mode(), TokenMode::Required);
}

fn request_from_fixture(source: &str) -> (DecisionRequest, Value) {
    let body = fixture(source);
    let question_body = &body["questions"]["q"];
    let question = match question_body["type"].as_str().unwrap() {
        "noul" => DecisionQuestion::Noul {
            criteria: question_body.get("criteria").map(|criteria| NoulCriteria {
                true_def: criteria["true"].as_str().unwrap().to_string(),
                false_def: criteria["false"].as_str().unwrap().to_string(),
            }),
        },
        "score" => DecisionQuestion::Score {
            levels: question_body["criteria"]
                .as_array()
                .unwrap()
                .iter()
                .map(|level| level.as_str().unwrap().to_string())
                .collect(),
        },
        "choice" => DecisionQuestion::Choice {
            options: question_body["criteria"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| (key.clone(), value.as_str().unwrap().to_string()))
                .collect(),
        },
        _ => unreachable!(),
    };
    (
        DecisionRequest {
            state: body["state"].clone(),
            instructions: question_body["instructions"].as_str().unwrap().to_string(),
            question,
        },
        body,
    )
}

#[test]
fn typesafe_request_noul_with_criteria_matches_fixture() {
    let (request, expected) = request_from_fixture(REQUEST_NOUL);
    assert_eq!(
        TypeSafeAdapter::build_request_body("jev-latest", &request),
        expected
    );
}

#[test]
fn typesafe_request_noul_plain_omits_criteria() {
    let (request, expected) = request_from_fixture(REQUEST_NOUL_PLAIN);
    let body = TypeSafeAdapter::build_request_body("jev-latest", &request);
    assert_eq!(body, expected);
    assert!(body["state"].is_string());
    assert!(body["questions"]["q"].get("criteria").is_none());
}

#[test]
fn typesafe_request_state_object_round_trips() {
    let (request, expected) = request_from_fixture(REQUEST_NOUL);
    let body = TypeSafeAdapter::build_request_body("jev-latest", &request);
    assert!(request.state.is_object());
    assert_eq!(body["state"], request.state);
    assert_eq!(body, expected);
}
#[test]
fn typesafe_request_score_matches_fixture() {
    let (request, expected) = request_from_fixture(REQUEST_SCORE);
    assert_eq!(
        TypeSafeAdapter::build_request_body("jev-latest", &request),
        expected
    );
}
#[test]
fn typesafe_request_choice_matches_fixture() {
    let (request, expected) = request_from_fixture(REQUEST_CHOICE);
    assert_eq!(
        TypeSafeAdapter::build_request_body("jev-latest", &request),
        expected
    );
}

#[test]
fn typesafe_parse_noul_answer() {
    assert_eq!(
        TypeSafeAdapter::parse_answer(&question("noul"), &fixture(ANSWER_NOUL)),
        Ok(DecisionAnswer::Noul { p: 0.8 })
    );
}
#[test]
fn typesafe_parse_score_answer() {
    assert_eq!(
        TypeSafeAdapter::parse_answer(&question("score"), &fixture(ANSWER_SCORE)),
        Ok(DecisionAnswer::Score {
            score: 1.5,
            confidence: Some(0.7),
            probabilities: BTreeMap::from([
                ("0".into(), 0.1),
                ("1".into(), 0.3),
                ("2".into(), 0.6)
            ])
        })
    );
}
#[test]
fn typesafe_parse_choice_answer() {
    assert_eq!(
        TypeSafeAdapter::parse_answer(&question("choice"), &fixture(ANSWER_CHOICE)),
        Ok(DecisionAnswer::Choice {
            choice: "billing".into(),
            confidence: Some(0.8),
            probabilities: BTreeMap::from([("billing".into(), 0.9), ("support".into(), 0.1)])
        })
    );
}
#[test]
fn typesafe_parse_score_answer_without_confidence() {
    let mut response = fixture(ANSWER_SCORE);
    response["answers"]["q"]
        .as_object_mut()
        .unwrap()
        .remove("confidence");
    response["answers"]["q"]["score"] = serde_json::json!(3.75);
    assert_eq!(
        TypeSafeAdapter::parse_answer(&question("score"), &response),
        Ok(DecisionAnswer::Score {
            score: 3.75,
            confidence: None,
            probabilities: BTreeMap::from([
                ("0".into(), 0.1),
                ("1".into(), 0.3),
                ("2".into(), 0.6),
            ]),
        })
    );
}

#[test]
fn typesafe_parse_score_answer_partial_levels() {
    let mut response = fixture(ANSWER_SCORE);
    response["answers"]["q"]["probabilities"] = serde_json::json!({"2": 0.6});
    assert_eq!(
        TypeSafeAdapter::parse_answer(&question("score"), &response),
        Ok(DecisionAnswer::Score {
            score: 1.5,
            confidence: Some(0.7),
            probabilities: BTreeMap::from([("2".into(), 0.6)]),
        })
    );
}

#[test]
fn typesafe_parse_choice_answer_partial_probabilities() {
    let mut response = fixture(ANSWER_CHOICE);
    response["answers"]["q"]["probabilities"] = serde_json::json!({"billing": 0.9});
    response["answers"]["q"]
        .as_object_mut()
        .unwrap()
        .remove("confidence");
    assert_eq!(
        TypeSafeAdapter::parse_answer(&question("choice"), &response),
        Ok(DecisionAnswer::Choice {
            choice: "billing".into(),
            confidence: None,
            probabilities: BTreeMap::from([("billing".into(), 0.9)]),
        })
    );
}

#[test]
fn typesafe_malformed_answer_is_parse_error() {
    assert!(matches!(
        TypeSafeAdapter::parse_answer(&question("score"), &fixture(ANSWER_MALFORMED)),
        Err(DecisionError::Parse(_))
    ));
}

fn adapter(endpoint: String, with_token: bool) -> (tempfile::TempDir, TypeSafeAdapter) {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::new(dir.path().join("tokens"));
    if with_token {
        store.set(TypeSafeAdapter::ID, TOKEN).unwrap();
    }
    (dir, TypeSafeAdapter::new(store, Some(endpoint), None))
}

#[test]
fn typesafe_adapter_builds_http_client_before_its_first_decision() {
    let (_dir, adapter) = adapter("http://127.0.0.1:1/".into(), false);

    assert!(adapter.client.is_ok());
}

fn read_head(stream: &mut std::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let count = stream.read(&mut buf).unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buf[..count]);
        if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    String::from_utf8(bytes).unwrap()
}

#[test]
fn typesafe_sends_bearer_header() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (_dir, adapter) = adapter(format!("http://{}/", listener.local_addr().unwrap()), true);
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let head = read_head(&mut stream);
        assert!(head.starts_with("POST / HTTP/1.1"));
        assert!(head
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {TOKEN}")));
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            ANSWER_NOUL.len(),
            ANSWER_NOUL
        )
        .unwrap();
    });
    assert_eq!(
        adapter.decide(&request("noul"), Instant::now() + Duration::from_secs(2)),
        Ok(DecisionAnswer::Noul { p: 0.8 })
    );
    server.join().unwrap();
}

#[test]
fn typesafe_non_2xx_maps_http_status() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (_dir, adapter) = adapter(format!("http://{}/", listener.local_addr().unwrap()), true);
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_head(&mut stream);
        stream
            .write_all(
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .unwrap();
    });
    assert_eq!(
        adapter.decide(&request("noul"), Instant::now() + Duration::from_secs(2)),
        Err(DecisionError::Http(401))
    );
    server.join().unwrap();
}

#[test]
fn typesafe_unresponsive_endpoint_hits_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let (_dir, adapter) = adapter(format!("http://{}/", listener.local_addr().unwrap()), true);
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_head(&mut stream);
        rx.recv().unwrap();
    });
    let start = Instant::now();
    let result = adapter.decide(&request("noul"), start + Duration::from_millis(300));
    tx.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(result, Err(DecisionError::Deadline));
    assert!(start.elapsed() < Duration::from_millis(1300));
}

#[test]
fn typesafe_missing_token_is_unavailable_without_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let (_dir, adapter) = adapter(format!("http://{}/", listener.local_addr().unwrap()), false);
    assert_eq!(
        adapter.decide(&request("noul"), Instant::now() + Duration::from_secs(1)),
        Err(DecisionError::Unavailable("token_missing".into()))
    );
    assert!(matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
}

#[test]
fn typesafe_models_list_the_default_plus_a_configured_jev_version() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = TypeSafeAdapter::new(
        TokenStore::new(dir.path().join("tokens")),
        None,
        Some("jev-1.13.0".into()),
    );
    let ids: Vec<String> = adapter.models().into_iter().map(|model| model.id).collect();
    assert_eq!(ids, ["jev-latest", "jev-1.13.0"]);
}

#[test]
fn typesafe_ignores_another_adapters_model_and_keeps_jev_selectable() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = TypeSafeAdapter::new(
        TokenStore::new(dir.path().join("tokens")),
        None,
        Some("local-decision-model".into()),
    );
    let ids: Vec<String> = adapter.models().into_iter().map(|model| model.id).collect();
    assert_eq!(ids, ["jev-latest"]);
    assert!(!adapter.select_model("local-decision-model"));
    assert!(adapter.select_model("jev-1.13.0"));
    assert_eq!(adapter.model_id().as_deref(), Some("jev-1.13.0"));
}

fn multi_request() -> DecisionQuestionsRequest {
    DecisionQuestionsRequest {
        state: serde_json::json!({ "fixture": "shared" }),
        questions: BTreeMap::from([
            (
                "label".into(),
                NamedDecisionQuestion {
                    instructions: "opaque-label-instructions".into(),
                    question: DecisionQuestion::Choice {
                        options: BTreeMap::from([
                            ("yes".into(), "opaque-yes".into()),
                            ("no".into(), "opaque-no".into()),
                        ]),
                    },
                },
            ),
            (
                "prompt_visible".into(),
                NamedDecisionQuestion {
                    instructions: "opaque-prompt-instructions".into(),
                    question: DecisionQuestion::Noul { criteria: None },
                },
            ),
            (
                "relevance".into(),
                NamedDecisionQuestion {
                    instructions: "opaque-score-instructions".into(),
                    question: DecisionQuestion::Score {
                        levels: vec!["opaque-low".into(), "opaque-high".into()],
                    },
                },
            ),
        ]),
    }
}

#[test]
fn typesafe_multi_question_request_matches_fixture() {
    assert_eq!(
        TypeSafeAdapter::build_questions_body("jev-latest", &multi_request()),
        fixture(include_str!("../fixtures/request_multi.json"))
    );
}

#[test]
fn typesafe_usage_parsing_and_declared_pricing() {
    assert_eq!(
        TypeSafeAdapter::parse_usage(
            &serde_json::json!({ "usage": { "input_tokens": 1200, "output_tokens": 15 } })
        )
        .unwrap(),
        Some(DecisionUsage {
            input_tokens: 1200,
            output_tokens: 15
        })
    );
    assert_eq!(
        TypeSafeAdapter::parse_usage(&serde_json::json!({})).unwrap(),
        None
    );
    for invalid in [
        serde_json::json!({ "input_tokens": -1, "output_tokens": 3 }),
        serde_json::json!({ "input_tokens": 1 }),
    ] {
        assert!(TypeSafeAdapter::parse_usage(&serde_json::json!({ "usage": invalid })).is_err());
    }
    let (_dir, adapter) = adapter("http://127.0.0.1:1".into(), false);
    let price = adapter.pricing().unwrap();
    assert_eq!(
        price.estimated_usd(DecisionUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000
        }),
        Some(0.042)
    );
}

#[test]
fn typesafe_multi_answer_is_named_and_all_or_nothing() {
    let request = multi_request();
    let mut body = serde_json::json!({ "answers": {
        "label": { "type": "choice", "choice": "yes", "probabilities": { "yes": 0.8, "no": 0.2 } },
        "prompt_visible": { "type": "noul", "noul": 0.7 },
        "relevance": { "type": "score", "score": 1, "probabilities": { "0": 0.2, "1": 0.8 } }
    }});
    let answers = TypeSafeAdapter::parse_answers(&request, &body).unwrap();
    assert_eq!(answers.len(), 3);
    assert_eq!(answers["prompt_visible"], DecisionAnswer::Noul { p: 0.7 });
    body["answers"].as_object_mut().unwrap().remove("label");
    assert!(TypeSafeAdapter::parse_answers(&request, &body).is_err());
    body["answers"]["unexpected"] = serde_json::json!({ "type": "noul", "noul": 0.9 });
    assert!(TypeSafeAdapter::parse_answers(&request, &body).is_err());
}

#[test]
fn typesafe_returns_usage_with_one_multi_request_and_credit_errors() {
    use codeterm_decision::physical;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Meter(AtomicUsize);
    impl physical::RequestObserver for Meter {
        fn before_request(&self) -> Result<(), DecisionError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let request = DecisionQuestionsRequest {
        state: serde_json::json!("shared"),
        questions: BTreeMap::from([
            (
                "first".into(),
                NamedDecisionQuestion {
                    instructions: "a".into(),
                    question: question("noul"),
                },
            ),
            (
                "second".into(),
                NamedDecisionQuestion {
                    instructions: "b".into(),
                    question: question("noul"),
                },
            ),
        ]),
    };
    for (status, body) in [
        (
            200,
            serde_json::json!({ "answers": { "first": { "type": "noul", "noul": 0.8 }, "second": { "type": "noul", "noul": 0.4 } }, "usage": { "input_tokens": 123, "output_tokens": 45 } }),
        ),
        (402, serde_json::json!({})),
        (
            403,
            serde_json::json!({ "error": { "code": "out_of_credits" } }),
        ),
        (
            200,
            serde_json::json!({ "error": { "code": "insufficient_credits" } }),
        ),
    ] {
        let meter = Arc::new(Meter(AtomicUsize::new(0)));
        let _scope = physical::RequestScope::enter(meter.clone());
        let credit_error = status == 402 || body.get("error").is_some();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let (_dir, adapter) = adapter(format!("http://{}/", listener.local_addr().unwrap()), true);
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_head(&mut stream);
            let body = body.to_string();
            write!(stream, "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let response = adapter.decide_questions(&request, Instant::now() + Duration::from_secs(2));
        server.join().unwrap();
        assert_eq!(meter.0.load(Ordering::SeqCst), 1);
        if credit_error {
            assert_eq!(response, Err(DecisionError::CreditExhausted));
        } else {
            let response = response.unwrap();
            assert_eq!(response.answers.len(), 2);
            assert_eq!(
                response.usage,
                Some(DecisionUsage {
                    input_tokens: 123,
                    output_tokens: 45
                })
            );
        }
    }
}

#[test]
fn typesafe_physical_request_missing_token_does_not_count() {
    use codeterm_decision::physical;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct Meter(AtomicUsize);
    impl physical::RequestObserver for Meter {
        fn before_request(&self) -> Result<(), DecisionError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let meter = Arc::new(Meter(AtomicUsize::new(0)));
    let _scope = physical::RequestScope::enter(meter.clone());
    let (_dir, model) = adapter("http://127.0.0.1:1/".into(), false);
    assert!(model.supports_question_batch());
    assert!(model
        .decide(&request("noul"), Instant::now() + Duration::from_secs(2))
        .is_err());
    assert_eq!(meter.0.load(Ordering::SeqCst), 0);
}
