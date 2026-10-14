use super::*;
use crate::service::structured_prompt::opencode_sse::{parse_event, ParsedEvent};

fn route(questions: Value) -> ResponseRoute {
    let ParsedEvent::Prompt { route, .. } = parse_event(&json!({
        "type":"question.asked", "properties":{"id":"q", "sessionID":"s", "questions":questions}
    })) else {
        panic!("expected question route");
    };
    route
}

#[test]
fn answers_every_question_and_submits_every_selected_label_once() {
    let route = route(json!([
        {"header":"one", "question":"first", "custom":false, "options":[{"label":"A"},{"label":"B"}]},
        {"header":"two", "question":"second", "multiple":true, "options":[{"label":"C"},{"label":"D"},{"label":"E"}]}
    ]));
    let Answer::Next {
        route: next,
        prompt,
    } = answer(&route, "2").unwrap()
    else {
        panic!("expected next question");
    };
    assert!(route.answers.is_empty());
    assert_eq!(next.answers, vec![vec!["B"]]);
    assert_eq!(prompt.question, "second");
    let sections = prompt.sections.unwrap();
    assert!(sections[0].completed);
    assert!(sections[1].active);
    let Answer::Complete { body, .. } = answer(&next, "10005").unwrap() else {
        panic!("expected final reply");
    };
    assert_eq!(body, json!({"answers":[["B"],["C","E"]]}));
    assert_eq!(next.answers, vec![vec!["B"]]);
}

#[test]
fn invalid_selection_never_advances_and_failed_final_reply_can_retry() {
    let route = route(json!([
        {"question":"first", "custom":false, "options":[{"label":"A"}]},
        {"question":"second", "custom":false, "multiple":true, "options":[{"label":"B"}]}
    ]));
    assert!(answer(&route, "2").is_none());
    assert!(answer(&route, "unknown").is_none());
    assert!(answer(&route, "9000").is_none());
    let Answer::Next { route: next, .. } = answer(&route, "1").unwrap() else {
        panic!("expected next");
    };
    assert!(answer(&next, "10002").is_none());
    let Answer::Complete { body, .. } = answer(&next, "10001").unwrap() else {
        panic!("expected complete");
    };
    assert_eq!(body, json!({"answers":[["A"],["B"]]}));
    assert_eq!(next.answers.len(), 1);
    let Answer::Complete { body: retry, .. } = answer(&next, "10001").unwrap() else {
        panic!("expected retry");
    };
    assert_eq!(retry, body);
}

#[test]
fn multi_select_accepts_empty_selection_and_json_values_without_bit_limits() {
    let route = route(json!([{"question":"many", "multiple":true,"custom":false,
        "options":(0..40).map(|index| json!({"label":format!("choice-{index}")})).collect::<Vec<_>>()
    }]));
    let Answer::Complete { body, .. } = answer(&route, r#"["1","40","1"]"#).unwrap() else {
        panic!("expected reply");
    };
    assert_eq!(body, json!({"answers":[["choice-0","choice-39"]]}));
    assert!(answer(&route, "10001").is_none());
    let Answer::Complete { body, .. } = answer(&route, "[]").unwrap() else {
        panic!("expected empty selection reply");
    };
    assert_eq!(body, json!({"answers":[[]]}));
}

#[test]
fn custom_answers_follow_the_question_contract() {
    let route = route(json!([{"question":"write", "options":[]}]));
    let prompt = prompt(&route).unwrap();
    assert_eq!(prompt.options[0].value, "custom");
    assert!(!prompt.submit_available.unwrap());
    let Answer::Complete { body, .. } = answer(&route, "opaque input").unwrap() else {
        panic!("expected custom reply");
    };
    assert_eq!(body, json!({"answers":[["opaque input"]]}));
    assert!(answer(&route, "").is_none());
    assert!(answer(&route, "custom").is_none());
}
