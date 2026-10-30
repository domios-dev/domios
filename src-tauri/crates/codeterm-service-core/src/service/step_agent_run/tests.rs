use super::*;

fn message(kind: &str, content: &str) -> ChatMsg {
    serde_json::from_value(serde_json::json!({"id":content,"type":kind,"content":content})).unwrap()
}

#[test]
fn step_run_reply_is_the_final_assistant_message_and_is_bounded() {
    let turn = [message("user", "ask"), message("assistant", " answer ")];
    assert_eq!(final_reply(&turn).as_deref(), Some("answer"));
    assert_eq!(final_reply(&[message("assistant", "a"), message("tool_result", "x")]), None);
    assert_eq!(final_reply(&[message("assistant", "  ")]), None);
    let long = "é".repeat(MAX_REPLY_BYTES);
    let reply = final_reply(&[message("assistant", &long)]).unwrap();
    assert!(reply.len() <= MAX_REPLY_BYTES && reply.ends_with(TRUNCATED));
}

#[test]
fn step_run_cwd_refuses_relative_missing_and_file_paths() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(working_dir("relative/dir").is_err());
    assert!(working_dir(&dir.path().join("missing").to_string_lossy()).is_err());
    assert!(working_dir(&file.to_string_lossy()).is_err());
    let shown = working_dir(&dir.path().to_string_lossy()).unwrap();
    assert!(!shown.starts_with(r"\\?\"));
}
