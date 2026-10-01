use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::super::NoulCriteria;
use super::*;

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
fn typesafe_models_returns_configured_model() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = TypeSafeAdapter::new(
        TokenStore::new(dir.path().join("tokens")),
        None,
        Some("fixture-model".into()),
    );
    assert_eq!(
        adapter.models(),
        vec![DecisionModelInfo {
            id: "fixture-model".into(),
            display_name: "fixture-model".into(),
        }]
    );
}
