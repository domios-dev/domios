use super::*;
use serde_json::json;

#[test]
fn codex_rpc_hot_resume_requires_read_verification() {
    assert_eq!(
        selection_for_event("old", &json!({"threadId":"new"})),
        Selection::Verify("new".into())
    );
    assert_eq!(
        selection_for_event("old", &json!({"threadId":"old"})),
        Selection::Current
    );
    assert_eq!(
        selection_for_event("old", &json!({"item":{"threadId":"nested"}})),
        Selection::Ignore
    );
}

#[test]
fn codex_rpc_thread_read_rejects_children_and_unknown_sources() {
    let user =
        json!({"thread":{"id":"new","threadSource":"user","source":"cli","parentThreadId":null}});
    assert!(verified_user_thread("new", &user));
    assert!(!verified_user_thread("other", &user));
    for child in [
        json!({"thread":{"id":"new","threadSource":"subagent","parentThreadId":null}}),
        json!({"thread":{"id":"new","threadSource":"user","parentThreadId":"root"}}),
        json!({"thread":{"id":"new","threadSource":"user","source":{"subAgent":{"thread_spawn":{}}}}}),
        json!({"thread":{"id":"new"}}),
    ] {
        assert!(!verified_user_thread("new", &child));
    }
}
