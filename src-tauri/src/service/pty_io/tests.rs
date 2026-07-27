use super::send;
use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskLaneMetadata};
use crate::service::plan::{NewTask, PlanStore, TaskLaneUpdate};
use crate::service::pty_io::lifecycle::{active_tab_lane_base, tab_task_env};
use crate::service::pty_io::lifecycle::{
    claim_restored_launch, claim_restored_pty_boot, restore_and_schedule_launch,
    restore_materialization_core,
};

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
fn concurrent_restore_single_winner_cas() {
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use codeterm_util::lock_hold::TimedRwLock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    let (mut state, parent_id) = MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    state
        .set_tab_role(&parent_id, AgentRole::Orchestrator, None)
        .unwrap();
    let team_id = state
        .resolve_or_mint_team_for_parent(&parent_id)
        .unwrap()
        .team_id;
    let tab_id = state.create_tab(&workspace_id).unwrap().1;
    state
        .set_tab_role(&tab_id, AgentRole::Worker, Some(&parent_id))
        .unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("fixture-session".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.restored_from_snapshot = true;
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-test", state));
    let barrier = Arc::new(Barrier::new(3));
    let schedules = Arc::new(AtomicUsize::new(0));
    let results = (0..2)
        .map(|_| {
            let mux = mux.clone();
            let tab_id = tab_id.clone();
            let barrier = barrier.clone();
            let schedules = schedules.clone();
            std::thread::spawn(move || {
                barrier.wait();
                restore_and_schedule_launch(
                    &mux,
                    &tab_id,
                    "fixture-provider",
                    Some("fixture-session"),
                    Some("fixture-cwd"),
                    true,
                    None,
                    |_, _| {
                        schedules.fetch_add(1, Ordering::SeqCst);
                    },
                )
                .unwrap()
                .is_some()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    assert_eq!(
        results
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|won| *won)
            .count(),
        1
    );
    assert_eq!(schedules.load(Ordering::SeqCst), 1);
    let tab = mux.read().get_tab(&tab_id).unwrap().clone();
    assert_eq!(tab.parent_tab_id.as_deref(), Some(parent_id.as_str()));
    assert_eq!(tab.team_id.as_deref(), Some(team_id.as_str()));
    assert_eq!(tab.agent_session_id.as_deref(), Some("fixture-session"));
}

#[test]
fn restore_resolves_payload_into_launch_option() {
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_util::lock_hold::TimedRwLock;
    use sha2::Digest;
    use std::sync::Arc;

    let payload = "byte-identical restored fallback brief";
    let digest = format!("{:x}", sha2::Sha256::digest(payload.as_bytes()));
    let temp = tempfile::tempdir().unwrap();
    let store = crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
        temp.path(),
        &std::collections::HashSet::new(),
    )
    .unwrap();
    store.write(&digest, payload).unwrap();
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.restored_from_snapshot = true;
    tab.pending_delivery = Some(crate::multiplexer::types::PendingDeliveryRecord::new(
        digest.clone(),
        crate::multiplexer::types::PendingDeliveryChannel::InteractiveInput,
    ));
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-payload-test", state));
    let resolved = store.resolve(&digest).unwrap();
    let observed = std::sync::Arc::new(parking_lot::Mutex::new(None));
    let sink = observed.clone();

    restore_and_schedule_launch(
        &mux,
        &tab_id,
        "fixture-provider",
        None,
        Some("fixture-cwd"),
        false,
        Some(resolved),
        move |_, task| *sink.lock() = task,
    )
    .unwrap();

    assert_eq!(observed.lock().as_deref(), Some(payload));
}

#[test]
fn restore_missing_payload_spawnfailed_zero_launch_pending() {
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::{
        PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
    };
    use crate::service::boot_stage::BootStage;
    use codeterm_util::lock_hold::TimedRwLock;
    use sha2::Digest;
    use std::sync::Arc;

    let payload = "missing restored fallback brief";
    let digest = format!("{:x}", sha2::Sha256::digest(payload.as_bytes()));
    let temp = tempfile::tempdir().unwrap();
    let store = crate::service::pending_delivery_payloads::PendingDeliveryPayloadStore::open(
        temp.path(),
        &std::collections::HashSet::new(),
    )
    .unwrap();
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.cwd = Some("fixture-cwd".into());
    tab.restored_from_snapshot = true;
    tab.pending_delivery = Some(PendingDeliveryRecord::new(
        digest.clone(),
        PendingDeliveryChannel::InteractiveInput,
    ));
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-missing-payload-test", state));
    let launch_request = parking_lot::Mutex::new(None);
    let boot_failure = parking_lot::Mutex::new(None);

    let result = restore_materialization_core(
        &mux,
        &tab_id,
        "fixture-provider",
        None,
        Some("fixture-cwd"),
        false,
        false,
        || {
            store
                .resolve(&digest)
                .map(Some)
                .map_err(|error| error.to_string())
        },
        |stage, reason| *boot_failure.lock() = Some((stage, reason.to_string())),
        |launch, payload| *launch_request.lock() = Some((launch, payload)),
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("missing payload must prevent restored-agent launch"),
    };

    assert_eq!(error, format!("RestorePayloadMissing: {digest}"));
    assert_eq!(
        *boot_failure.lock(),
        Some((BootStage::SpawnFailed, error.clone()))
    );
    assert!(launch_request.lock().is_none());
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .pending_delivery
            .as_ref()
            .unwrap()
            .state(),
        PendingDeliveryState::Pending
    );
}

#[test]
fn stale_session_validity_cannot_claim_changed_tab() {
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_util::lock_hold::TimedRwLock;
    use std::sync::Arc;
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("new".into());
    tab.cwd = Some("cwd".into());
    tab.restored_from_snapshot = true;
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-test", state));
    assert!(claim_restored_launch(
        &mux,
        &tab_id,
        "fixture-provider",
        Some("old"),
        Some("cwd"),
        true
    )
    .unwrap()
    .is_none());
    assert!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .restored_from_snapshot
    );
}

#[test]
fn restored_pty_boot_claim_excludes_lazy_materialization() {
    use crate::multiplexer::commands::MuxState;
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_util::lock_hold::TimedRwLock;
    use std::sync::Arc;
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("fixture-provider".into());
    tab.agent_session_id = Some("session".into());
    tab.cwd = Some("cwd".into());
    tab.restored_from_snapshot = true;
    let mux: MuxState = Arc::new(TimedRwLock::new("restore-boot-claim-test", state));

    let claim = claim_restored_pty_boot(&mux, &tab_id).unwrap().unwrap();
    assert_eq!(claim.session_id.as_deref(), Some("session"));
    assert!(claim_restored_launch(
        &mux,
        &tab_id,
        "fixture-provider",
        Some("session"),
        Some("cwd"),
        true,
    )
    .unwrap()
    .is_none());
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
    let message = "-=-codeterm:tab label=worker tab=abc123 model=codex-=-run tests";
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

/// Routing rule: a shell tab (no agent_type) must use script-exec, not
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
fn windows_spawn_dispatch_prepends_sacrificial_space_without_glued_enter() {
    let bytes = build_spawn_dispatch("Remove-Item Env:CLAUDECODE; claude").unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert_eq!(s, "\x1b[200~ Remove-Item Env:CLAUDECODE; claude\x1b[201~");
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
        OrchestratorRef::from_tab("orch"),
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
        .assign_task(&plan.id, &task_id, "tab-a", None, 3)
        .unwrap();
    (dir, store, plan.id, task_id)
}

#[test]
fn active_tab_lane_base_prefers_live_attempt_base() {
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
        active_tab_lane_base(&store, "tab-a").as_deref(),
        Some("attempt-base")
    );
}

#[test]
fn active_tab_lane_base_falls_back_to_lane_base_when_attempt_missing() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task("lane-base");

    assert_eq!(
        active_tab_lane_base(&store, "tab-a").as_deref(),
        Some("lane-base")
    );
}

#[test]
fn active_tab_lane_base_ignores_closed_and_other_tab_attempts() {
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
        .assign_task(&plan_id, &task_id, "tab-b", None, 6)
        .unwrap();
    store
        .assign_task(&plan_id, &task_id, "tab-a", None, 7)
        .unwrap();

    assert_eq!(
        active_tab_lane_base(&store, "tab-a").as_deref(),
        Some("lane-base")
    );
    assert_eq!(active_tab_lane_base(&store, "tab-b"), None);
}

#[test]
fn active_tab_lane_base_none_without_binding() {
    let (_dir, store, _plan_id, _task_id) = store_with_bound_task("lane-base");

    assert_eq!(active_tab_lane_base(&store, "unbound-tab"), None);
}

#[test]
fn tab_task_env_uses_live_attempt_base_and_leaves_plain_tab_clean() {
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

    let bound = tab_task_env(&store, "tab-a");
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

    let plain = tab_task_env(&store, "plain-tab");
    assert!(
        plain.is_empty(),
        "unbound Tabs must not inherit plan/task/lane env"
    );
}
