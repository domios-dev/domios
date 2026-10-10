use super::*;

#[test]
fn parity_account_home_claude_session_path_uses_inherited_root_without_writes() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    let home = tempfile::tempdir().unwrap();
    env.set("CLAUDE_CONFIG_DIR", home.path());
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    let session_id = "00000000-0000-0000-0000-000000000001";
    let expected = home
        .path()
        .join("projects")
        .join("-work")
        .join(format!("{session_id}.jsonl"));
    let native_path = PathBuf::from(native.session_file_path("/work", session_id).unwrap());
    let js_path = PathBuf::from(js.session_file_path("/work", session_id).unwrap());
    assert_eq!(native_path, expected);
    assert_eq!(js_path, expected);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}
