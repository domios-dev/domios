use super::send;
use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
use crate::service::plan::{NewTask, PlanStore, TaskLaneUpdate};
use crate::service::pty_io::lifecycle::{active_pane_lane_base, pane_task_env};

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
        restored_agent_materialization(true, false, false, Some("codex"), Some("session-1"), true),
        Some(RestoredAgentMaterialization::Resume)
    );
}

#[test]
fn restored_agent_without_valid_session_needs_fresh_materialization() {
    assert_eq!(
        restored_agent_materialization(true, false, false, Some("codex"), Some("missing"), false),
        Some(RestoredAgentMaterialization::Fresh)
    );
    assert_eq!(
        restored_agent_materialization(true, false, false, Some("codex"), None, false),
        Some(RestoredAgentMaterialization::Fresh)
    );
}

#[test]
fn live_agent_send_skips_materialization() {
    assert_eq!(
        restored_agent_materialization(false, false, false, Some("codex"), Some("session-1"), true),
        None
    );
}

#[test]
fn send_during_snapshot_resume_joins_existing_boot() {
    assert_eq!(
        restored_agent_materialization(true, true, false, Some("codex"), Some("session-1"), true),
        None
    );
}

#[test]
fn plain_shell_send_skips_materialization() {
    assert_eq!(
        restored_agent_materialization(true, false, false, None, None, false),
        None
    );
}

#[test]
fn protected_agent_send_skips_materialization() {
    assert_eq!(
        restored_agent_materialization(true, false, true, Some("codex"), Some("session-1"), true),
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
fn agent_prompt_sanitizer_strips_escaped_ansi_and_controls() {
    let leaked = "\x1b]10;rgb:c9c9/d1d1/d9d9\x1b\\ok\x1b[?1;2c\x00!";
    assert_eq!(send::sanitize_agent_prompt_input(leaked), "ok!");
}

#[test]
fn agent_prompt_sanitizer_preserves_literal_reply_text() {
    let text = "]10;rgb:c9c9/d1d1/d9d9\\ [?1;2c";
    assert_eq!(send::sanitize_agent_prompt_input(text), text);
}

#[test]
fn agent_prompt_sanitizer_preserves_codeterm_prefix_text() {
    let message = "-=-codeterm:pane label=worker pane=abc123 model=codex-=-run tests";
    assert_eq!(send::sanitize_agent_prompt_input(message), message);
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

/// Script-exec path must not contain bracketed-paste markers on Unix, else bare
/// shells (before DECSET ?2004 arms) render the markers as literal text.
#[cfg(unix)]
#[test]
fn unix_spawn_dispatch_has_no_bracketed_paste_markers() {
    let bytes = build_spawn_dispatch("echo hello; exec zsh").unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(
        !s.contains("\x1b[200~"),
        "Unix script-exec must not emit BP opener: {s:?}"
    );
    assert!(
        !s.contains("\x1b[201~"),
        "Unix script-exec must not emit BP closer: {s:?}"
    );
    assert!(
        s.ends_with('\r'),
        "dispatch bytes must end with CR to submit: {s:?}"
    );
}

/// Routing rule: a shell pane (no agent_type) must use script-exec, not
/// bracketed paste. Verified by checking build_spawn_dispatch output on Unix.
#[cfg(unix)]
#[test]
fn unix_spawn_dispatch_contains_bash_script_path() {
    let bytes = build_spawn_dispatch("echo test").unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(
        s.starts_with("/bin/bash /tmp/codeterm-spawn-"),
        "Unix dispatch must invoke temp script: {s:?}"
    );
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

fn store_with_bound_task(lane_base: &str) -> (tempfile::TempDir, PlanStore, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Lane base".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_pane("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "bound".into(),
                kind: "implement".into(),
                lane: TaskLaneMetadata {
                    base: Some(lane_base.into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task(&plan.id, &task_id, "pane-a", None, 3)
        .unwrap();
    (dir, store, plan.id, task_id)
}

#[test]
fn active_pane_lane_base_prefers_live_attempt_base() {
    let (_dir, store, plan_id, task_id) = store_with_bound_task("attempt-base");
    store
        .update_task_lane(
            &plan_id,
            &task_id,
            TaskLaneUpdate {
                base: Some(Some("lane-base".into())),
                ..Default::default()
            },
            4,
        )
        .unwrap();

    assert_eq!(
        active_pane_lane_base(&store, "pane-a").as_deref(),
        Some("attempt-base")
    );
}

#[test]
fn active_pane_lane_base_falls_back_to_lane_base_when_attempt_missing() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task("lane-base");

    assert_eq!(
        active_pane_lane_base(&store, "pane-a").as_deref(),
        Some("lane-base")
    );
}

#[test]
fn active_pane_lane_base_ignores_closed_and_other_pane_attempts() {
    let (_dir, store, plan_id, task_id) = store_with_bound_task("closed-base");
    store.revert_task_dispatch(&plan_id, &task_id, 4).unwrap();
    store
        .update_task_lane(
            &plan_id,
            &task_id,
            TaskLaneUpdate {
                base: Some(Some("lane-base".into())),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "pane-b", None, 6)
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "pane-a", None, 7)
        .unwrap();

    assert_eq!(
        active_pane_lane_base(&store, "pane-a").as_deref(),
        Some("lane-base")
    );
    assert_eq!(active_pane_lane_base(&store, "pane-b"), None);
}

#[test]
fn active_pane_lane_base_none_without_binding() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task("lane-base");

    assert_eq!(active_pane_lane_base(&store, "unbound-pane"), None);
}

#[test]
fn pane_task_env_uses_live_attempt_base_and_leaves_plain_pane_clean() {
    let (_dir, store, plan_id, task_id) = store_with_bound_task("attempt-base");
    store
        .update_task_lane(
            &plan_id,
            &task_id,
            TaskLaneUpdate {
                base: Some(Some("lane-base".into())),
                ..Default::default()
            },
            4,
        )
        .unwrap();

    let bound = pane_task_env(&store, "pane-a");
    assert_eq!(
        bound.get("CODETERM_LANE_BASE").map(String::as_str),
        Some("attempt-base")
    );
    assert_eq!(
        bound.get("CODETERM_PLAN_ID").map(String::as_str),
        Some(plan_id.as_str())
    );
    assert_eq!(
        bound.get("CODETERM_TASK_ID").map(String::as_str),
        Some(task_id.as_str())
    );

    let plain = pane_task_env(&store, "plain-pane");
    assert!(
        plain.is_empty(),
        "unbound panes must not inherit plan/task/lane env"
    );
}
