use super::*;

fn apply_enter(text: &str, enter: bool) -> String {
    if enter && !text.ends_with('\r') && !text.ends_with('\n') {
        format!("{}\r", text)
    } else {
        text.to_string()
    }
}

#[test]
fn send_created_agent_pty_uses_bounded_width() {
    assert_eq!(SEND_AGENT_BACKEND_COLS, 240);
    assert_eq!(SHELL_BACKEND_COLS, 120);
}

#[test]
fn restored_agent_with_valid_session_needs_resume_materialization() {
    assert_eq!(
        restored_agent_materialization(true, false, Some("codex"), Some("session-1"), true),
        Some(RestoredAgentMaterialization::Resume)
    );
}

#[test]
fn restored_agent_without_valid_session_needs_fresh_materialization() {
    assert_eq!(
        restored_agent_materialization(true, false, Some("codex"), Some("missing"), false),
        Some(RestoredAgentMaterialization::Fresh)
    );
    assert_eq!(
        restored_agent_materialization(true, false, Some("codex"), None, false),
        Some(RestoredAgentMaterialization::Fresh)
    );
}

#[test]
fn live_agent_send_skips_materialization() {
    assert_eq!(
        restored_agent_materialization(false, false, Some("codex"), Some("session-1"), true),
        None
    );
}

#[test]
fn plain_shell_send_skips_materialization() {
    assert_eq!(
        restored_agent_materialization(true, false, None, None, false),
        None
    );
}

#[test]
fn protected_agent_send_skips_materialization() {
    assert_eq!(
        restored_agent_materialization(true, true, Some("codex"), Some("session-1"), true),
        None
    );
}

#[test]
fn enter_true_appends_cr_when_missing() {
    assert_eq!(apply_enter("ls -la", true), "ls -la\r");
}

#[test]
fn enter_true_no_double_cr() {
    assert_eq!(apply_enter("ls -la\r", true), "ls -la\r");
}

#[test]
fn enter_false_passthrough() {
    assert_eq!(apply_enter("partial text", false), "partial text");
}

#[test]
fn enter_false_preserves_trailing_cr() {
    assert_eq!(apply_enter("text\r", false), "text\r");
}

#[test]
fn empty_text_enter_true_sends_just_cr() {
    assert_eq!(apply_enter("", true), "\r");
}

#[test]
fn empty_text_enter_false_sends_nothing() {
    assert_eq!(apply_enter("", false), "");
}

#[test]
fn enter_true_text_ending_newline() {
    assert_eq!(apply_enter("echo hello\n", true), "echo hello\n");
}

#[test]
fn text_with_embedded_cr_but_not_trailing() {
    assert_eq!(apply_enter("line1\rline2", true), "line1\rline2\r");
}

#[test]
fn multiline_text_enter_true() {
    assert_eq!(apply_enter("line1\nline2", true), "line1\nline2\r");
}

#[cfg(windows)]
#[test]
fn windows_spawn_dispatch_prepends_sacrificial_space() {
    let bytes = build_spawn_dispatch("Remove-Item Env:CLAUDECODE; claude").unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert_eq!(s, "\x1b[200~ Remove-Item Env:CLAUDECODE; claude\x1b[201~\r");
    let after_opener = &s["\x1b[200~".len()..];
    assert!(after_opener.starts_with(' '));
    assert!(after_opener[1..].starts_with("Remove-Item"));
}

#[test]
fn batch_item_enter_defaults_false() {
    let json = r#"{"text": "hello"}"#;
    let item: SendBatchItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.text, "hello");
    assert_eq!(
        item.enter, false,
        "SendBatchItem.enter defaults to false (serde default)"
    );
    assert!(item.delay_ms.is_none());
}

#[test]
fn batch_item_enter_explicit_true() {
    let json = r#"{"text": "cmd", "enter": true, "delay_ms": 100}"#;
    let item: SendBatchItem = serde_json::from_str(json).unwrap();
    assert!(item.enter);
    assert_eq!(item.delay_ms, Some(100));
}

#[test]
fn batch_item_enter_explicit_false() {
    let json = r#"{"text": "partial", "enter": false}"#;
    let item: SendBatchItem = serde_json::from_str(json).unwrap();
    assert!(!item.enter);
}

use crate::pty::screen_state::screen_has_shell_prompt;

#[test]
fn prompt_bash_dollar() {
    assert!(screen_has_shell_prompt("user@host:~$ "));
}

#[test]
fn prompt_bash_dollar_no_trailing_space() {
    assert!(screen_has_shell_prompt("user@host:~$"));
}

#[test]
fn prompt_zsh_percent() {
    assert!(screen_has_shell_prompt("user@host % "));
}

#[test]
fn prompt_powershell() {
    assert!(screen_has_shell_prompt("PS C:\\Users\\User> "));
}

#[test]
fn prompt_powershell_no_trailing_space() {
    assert!(screen_has_shell_prompt("PS C:\\Users\\User>"));
}

#[test]
fn prompt_generic_angle() {
    assert!(screen_has_shell_prompt("prompt> "));
}

#[test]
fn prompt_last_nonempty_line() {
    let screen = "some output\nmore output\nuser@host:~$ \n\n";
    assert!(screen_has_shell_prompt(screen));
}

#[test]
fn no_prompt_in_file_content() {
    let screen = "Reading file.txt:\ncontains PS data and > markers\nstill loading...";
    assert!(!screen_has_shell_prompt(screen));
}

#[test]
fn no_prompt_empty_screen() {
    assert!(!screen_has_shell_prompt(""));
    assert!(!screen_has_shell_prompt("\n\n\n"));
}

#[test]
fn no_false_positive_mid_line_dollar() {
    let screen = "echo $HOME is /home/user\nprocessing...";
    assert!(!screen_has_shell_prompt(screen));
}
