use super::*;
use serde_json::{json, Value};

fn page(items: Value, questions: Value) -> RuntimeResult {
    serde_json::from_value(json!({"kind":"historyPage","threadId":"thread","items":items,"openItems":[],"questions":questions})).unwrap()
}

#[test]
fn streamed_history_exceeds_state_limit_without_growing_continuation_state() {
    let mut history = RuntimeHistory::default();
    for seq in 0..80 {
        let state = json!({"stage":"items","seq":seq,"cursor":format!("page-{seq}")});
        validate_runtime_state(&state).unwrap();
        assert!(serde_json::to_vec(&state).unwrap().len() < 128);
        let chunk = page(
            json!([{"id":format!("item-{seq}"),"seq":seq,"type":"assistant","content":"x".repeat(8192)}]),
            Value::Null,
        );
        validate_runtime_size(&chunk, MAX_RUNTIME_VALUE_BYTES).unwrap();
        history.append(chunk);
    }
    let result = history.finish("thread".into(), None);
    assert!(serde_json::to_vec(&result).unwrap().len() > MAX_RUNTIME_STATE_BYTES);
    let RuntimeResult::HistoryPage { items, .. } = result else {
        panic!()
    };
    assert_eq!(items.len(), 80);
    assert_eq!(items.last().unwrap().seq, 79);
    assert_eq!(
        validate_runtime_state(&json!({"items":items}))
            .unwrap_err()
            .code,
        RuntimeFailureCode::SizeLimit
    );
}

#[test]
fn streamed_questions_require_exact_later_user_answers() {
    let question = json!([{ "threadId":"thread","itemId":"question","questions":[{"id":"q","text":"opaque","options":[{"value":"native","label":"Display"}]}],"response":"message" }]);
    for (kind, item_id, value, resolved) in [
        ("user", "question", "native", true),
        ("assistant", "question", "native", false),
        ("user", "wrong", "native", false),
        ("user", "question", "Display", false),
    ] {
        let mut history = RuntimeHistory::default();
        history.append(page(json!([]), question.clone()));
        let content =
            json!({"itemId":item_id,"answers":[{"questionId":"q","values":[value]}]}).to_string();
        history.append(page(
            json!([{"id":"answer","type":kind,"content":content}]),
            Value::Null,
        ));
        assert_eq!(history.questions()[0].resolved == Some(true), resolved);
    }
}

#[test]
fn repeated_history_request_is_admitted_only_after_a_nonempty_chunk() {
    let mut history = RuntimeHistory::default();
    let params = r#"{"threadId":"thread","cursor":"c1"}"#;
    assert!(history.admit_request("thread/items/list", params));
    assert!(!history.admit_request("thread/items/list", params));
    history.append(page(
        json!([{"id":"item","type":"assistant","content":"x"}]),
        Value::Null,
    ));
    assert!(history.admit_request("thread/items/list", params));
    history.append(page(json!([]), Value::Null));
    assert!(!history.admit_request("thread/items/list", params));
}
