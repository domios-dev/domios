use super::*;

#[test]
fn agent_resume_echoes_session_id() {
    let v: serde_json::Value = serde_json::from_str(&agent_resume_json("sess-7")).unwrap();
    assert_eq!(v["sessionId"], "sess-7");
}

#[test]
fn agent_get_is_null_host_keeps_no_key_map() {
    assert_eq!(agent_get_json(), "null");
}

#[test]
fn agent_usage_is_null_until_wired() {
    assert_eq!(agent_usage_json(), "null");
}
