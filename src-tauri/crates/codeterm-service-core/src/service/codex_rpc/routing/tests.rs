use super::*;
use serde_json::json;

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
