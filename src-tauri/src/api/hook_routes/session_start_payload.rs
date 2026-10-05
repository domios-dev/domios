use serde_json::Value;

pub(super) fn session_start_string(body: &Value, path: &[&str]) -> Option<String> {
    let mut cur = body;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub(super) fn session_start_session_id(body: &Value) -> Option<String> {
    session_start_string(body, &["session_id"])
        .or_else(|| session_start_string(body, &["payload", "session_id"]))
        .or_else(|| session_start_string(body, &["payload", "id"]))
        .or_else(|| session_start_string(body, &["sessionId"]))
}

pub(super) fn reported_transcript_path(body: &Value) -> Option<String> {
    session_start_string(body, &["transcript_path"])
        .or_else(|| session_start_string(body, &["payload", "transcript_path"]))
}
