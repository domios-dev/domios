use super::*;
use codeterm_util::lock_hold::TimedRwLock;

type TestMux = crate::multiplexer::commands::MuxState;

fn mux_with_nonce(nonce: Option<&str>) -> (TestMux, String) {
    let (state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let mux: TestMux = Arc::new(TimedRwLock::new("mux-test", state));
    if let Some(nonce) = nonce {
        mux.write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some(nonce.into());
    }
    (mux, tab_id)
}

fn add_tab_with_nonce(mux: &TestMux, sibling_tab: &str, nonce: &str) -> String {
    let workspace_id = mux.read().find_workspace_for_tab(sibling_tab).unwrap();
    let tab_id = mux.write().create_tab(&workspace_id).unwrap();
    mux.write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some(nonce.into());
    tab_id
}

#[test]
fn codex_stop_report_completes_only_the_nonce_matching_tab_run() {
    let (mux, tab_a) = mux_with_nonce(Some("nonce-a"));
    let tab_b = add_tab_with_nonce(&mux, &tab_a, "nonce-b");
    let registry = RunWatcherRegistry::new();
    let run_a = registry.insert_run_for_test("pty-a", &tab_a);
    let run_b = registry.insert_run_for_test("pty-b", &tab_b);

    let claimed = registry
        .claim_stop_report_run(&mux, &tab_a, "nonce-a")
        .expect("nonce-matching stop report claims the tab's run");
    assert_eq!(claimed.tab_id, tab_a);
    assert!(run_a.completed.load(Ordering::Acquire));
    assert!(
        !run_b.completed.load(Ordering::Acquire),
        "another tab's run must stay untouched"
    );
    assert!(
        registry
            .claim_stop_report_run(&mux, &tab_a, "nonce-a")
            .is_none(),
        "a claimed run is complete — a duplicate report claims nothing"
    );
}

#[test]
fn codex_stop_foreign_nonce_is_rejected_and_completes_nothing() {
    let (mux, tab_a) = mux_with_nonce(Some("nonce-a"));
    let tab_b = add_tab_with_nonce(&mux, &tab_a, "nonce-b");
    let registry = RunWatcherRegistry::new();
    let run_a = registry.insert_run_for_test("pty-a", &tab_a);
    let run_b = registry.insert_run_for_test("pty-b", &tab_b);

    // Another tab's nonce, an unknown nonce: neither claims anything.
    assert!(registry
        .claim_stop_report_run(&mux, &tab_a, "nonce-b")
        .is_none());
    assert!(registry
        .claim_stop_report_run(&mux, &tab_a, "nonce-unknown")
        .is_none());
    assert!(!run_a.completed.load(Ordering::Acquire));
    assert!(!run_b.completed.load(Ordering::Acquire));

    // A tab that never armed a nonce can't be completed via report at all.
    let (nonceless_mux, nonceless_tab) = mux_with_nonce(None);
    let run = registry.insert_run_for_test("pty-c", &nonceless_tab);
    assert!(registry
        .claim_stop_report_run(&nonceless_mux, &nonceless_tab, "nonce-a")
        .is_none());
    assert!(!run.completed.load(Ordering::Acquire));
}

#[test]
fn codex_stop_hook_channel_loss_leaves_classifier_fallback_completion_intact() {
    // DNC-12: the report path upgrades the signal, it never removes the safety net.
    let (mux, tab_id) = mux_with_nonce(Some("nonce-a"));
    let registry = RunWatcherRegistry::new();
    let run = registry.insert_run_for_test("pty-a", &tab_id);

    assert!(registry
        .claim_stop_report_run(&mux, &tab_id, "nonce-foreign")
        .is_none());
    assert!(
        !run.completed.load(Ordering::Acquire),
        "a rejected report must leave the run claimable by the fallback"
    );
    assert!(
        registry.runs.lock().contains_key("pty-a"),
        "the run stays registered for the classifier-driven watchdog"
    );
    // The classifier-fallback completion gate takes no hook input at all:
    // armed + saw_working + durable progress + classifier idle + bound.
    assert!(can_complete_on_semantic_idle(true, true, true, true, true));
    // Channel restored later: the same run is still completable by report.
    assert!(registry
        .claim_stop_report_run(&mux, &tab_id, "nonce-a")
        .is_some());
}

#[test]
fn screen_working_defers_idle_reap() {
    // The real frozen-spinner status line that used to get reaped mid-work.
    assert!(screen_shows_working(
        "✢ Propagating… (3m 18s · ↑ 9.8k tokens)\n❯ "
    ));
    assert!(screen_shows_working("✻ Schlepping… (12s · ↓ 2.1k tokens)"));
    assert!(screen_shows_working("working… esc to interrupt"));
}

#[test]
fn screen_working_false_on_idle_prompt() {
    assert!(!screen_shows_working(
        "╭─ Claude ─╮\n│ > │\n╰───╯  ? for shortcuts"
    ));
    assert!(!screen_shows_working("user@host:~$ "));
}

#[test]
fn bare_shell_detected_when_agent_gone() {
    assert!(screen_is_bare_shell("user@host:~/proj$ "));
    assert!(screen_is_bare_shell("PS C:\\Users\\foo> "));
}

#[test]
fn bare_shell_false_when_agent_visible() {
    assert!(!screen_is_bare_shell(
        "Welcome to Claude Code\nuser@host:~$ "
    ));
    assert!(!screen_is_bare_shell("thinking…  esc to interrupt"));
}

#[test]
fn bare_shell_false_on_empty_or_non_prompt() {
    assert!(!screen_is_bare_shell(""));
    assert!(!screen_is_bare_shell("   \n  "));
    assert!(!screen_is_bare_shell("still working on it\nno prompt here"));
}

#[test]
fn scanner_finds_result_event_inline() {
    let mut s = ResultEventScanner::new();
    let chunk = r#"some noise {"type":"system","x":1} more {"type":"result","subtype":"success","is_error":false,"result":"hello"}"#;
    let r = s.feed(chunk).expect("completion");
    assert_eq!(r.subtype.as_deref(), Some("success"));
    assert_eq!(r.is_error, false);
    assert_eq!(r.result.as_deref(), Some("hello"));
}

#[test]
fn scanner_ignores_mid_stream_message_stop() {
    // kills the tab before claude can process tool results.
    let mut s = ResultEventScanner::new();
    let chunk = r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"x"}}}
{"type":"stream_event","event":{"type":"message_stop"},"session_id":"a","uuid":"b"}"#;
    assert!(
        s.feed(chunk).is_none(),
        "must not complete on nested message_stop"
    );
}

#[test]
fn scanner_handles_split_across_chunks() {
    let mut s = ResultEventScanner::new();
    assert!(s.feed(r#"{"type":"result","is_error":false,"#).is_none());
    let r = s.feed(r#""result":"привет"}"#).expect("completion");
    assert_eq!(r.result.as_deref(), Some("привет"));
}

#[test]
fn scanner_strips_ansi() {
    let mut s = ResultEventScanner::new();
    let chunk =
        "\x1b[32m\x1b]0;title\x07{\"type\":\"result\",\"is_error\":true,\"result\":\"err\"}";
    let r = s.feed(chunk).expect("completion");
    assert!(r.is_error);
    assert_eq!(r.result.as_deref(), Some("err"));
}

#[test]
fn scanner_ignores_non_completion_events() {
    let mut s = ResultEventScanner::new();
    assert!(s
        .feed(r#"{"type":"assistant","message":{"id":"x"}}"#)
        .is_none());
    assert!(s
        .feed(r#"{"type":"stream_event","event":{"type":"content_block_delta"}}"#)
        .is_none());
}

#[test]
fn scanner_handles_braces_inside_strings() {
    let mut s = ResultEventScanner::new();
    let chunk = r#"{"type":"result","is_error":false,"result":"text with {nested} {{braces}} ok"}"#;
    let r = s.feed(chunk).expect("completion");
    assert_eq!(
        r.result.as_deref(),
        Some("text with {nested} {{braces}} ok")
    );
}

#[test]
fn scanner_caps_buffer_growth() {
    let mut s = ResultEventScanner::new();
    let big = "x".repeat(SCANNER_BUFFER_CAP * 2);
    let _ = s.feed(&big);
    assert!(s.buffer.len() <= SCANNER_BUFFER_CAP);
}

#[test]
fn scanner_caps_unicode_buffer_without_panic() {
    let mut s = ResultEventScanner::new();
    let big = "✶✳✻… ".repeat(SCANNER_BUFFER_CAP); // far over cap, all multibyte
    let _ = s.feed(&big);
    assert!(s.buffer.len() <= SCANNER_BUFFER_CAP);
    // Feeding more must keep working (no panic on the retained multibyte tail).
    let _ = s.feed("✢✽ more ✶");
    assert!(s.buffer.is_char_boundary(0));
}

#[test]
fn idle_status_is_terminal_for_any_provider() {
    assert!(status_is_idle(crate::agent::status::TabStatus::Idle));
    assert!(!status_is_idle(crate::agent::status::TabStatus::Working));
    assert!(!status_is_idle(crate::agent::status::TabStatus::Waiting));
}

#[test]
fn semantic_idle_requires_post_session_progress() {
    assert!(!can_complete_on_semantic_idle(
        false, false, false, true, true
    ));
    assert!(!can_complete_on_semantic_idle(
        true, false, false, true, true
    ));
    assert!(!can_complete_on_semantic_idle(
        true, true, false, true, true
    ));
    assert!(!can_complete_on_semantic_idle(
        true, true, true, false, true
    ));
    assert!(!can_complete_on_semantic_idle(
        true, true, true, true, false
    ));
    assert!(can_complete_on_semantic_idle(true, true, true, true, true));
}

#[test]
fn empty_assistant_metadata_is_not_durable_progress() {
    let empty = crate::agent::chat_types::StructuredChat {
        messages: vec![crate::agent::chat_types::ChatMsg {
            kind: "assistant".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(!has_nonempty_assistant_transcript(&empty));

    let completed = crate::agent::chat_types::StructuredChat {
        messages: vec![crate::agent::chat_types::ChatMsg {
            kind: "assistant".into(),
            content: "done".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert!(has_nonempty_assistant_transcript(&completed));
}

#[test]
fn hidden_agent_crash_recovery_uses_global_access_choice() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));

    let full_access = hidden_recovery_resume_command(&registry, "claude", "session-1", true, None)
        .expect("resume command");
    let restricted = hidden_recovery_resume_command(&registry, "claude", "session-1", false, None)
        .expect("resume command");

    assert!(full_access.contains("--dangerously-skip-permissions"));
    assert!(!restricted.contains("--dangerously-skip-permissions"));
}

#[test]
fn hidden_agent_crash_recovery_reapplies_the_registered_launch_context() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
    let context = "opaque-relaunch-context-3d9c";

    let command =
        hidden_recovery_resume_command(&registry, "claude", "session-1", false, Some(context))
            .expect("resume command with context");
    let path = command
        .split("--append-system-prompt-file ")
        .nth(1)
        .expect("system prompt file flag")
        .trim()
        .trim_matches('\'');
    let written = std::fs::read_to_string(path).expect("written relaunch context");

    assert_eq!(written.trim_start_matches('\u{feff}'), context);
}

#[test]
fn run_watcher_never_writes_keys() {
    let watcher = include_str!("../run_watcher.rs");

    assert!(!watcher.contains(".pty.lock().write"));
    assert!(!watcher.contains("b\"\\r\""));
}

fn owner_prompt_fixture() -> (crate::service::test_fixture::ServiceFixture, String, String) {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "cron-owner-prompt-pty".to_string();
    mux.link_pty(&tab_id, pty_id.clone());
    {
        let tab = mux.get_tab_mut(&tab_id).expect("prompt tab");
        tab.agent_type = Some("prompt-fixture".into());
        crate::service::boot_stage::apply_boot_stage(
            tab,
            crate::service::boot_stage::BootStage::AgentReady,
            None,
        );
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .worker_ack
        .supervise_startup_for_test(&tab_id, "prompt-owner");
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("prompt-fixture")
            .with_parsed_prompt(owner_prompt()),
    ));
    fixture
            .service
            .screen_state
            .lock()
            .process(
                &pty_id,
                "opaque-question-62\r\n❯ 1. opaque-choice-62\r\n  2. opaque-choice-63\r\nPress Enter to confirm"
                    .as_bytes(),
            );
    (fixture, tab_id, pty_id)
}

fn owner_prompt() -> codeterm_types::pty::PendingPrompt {
    codeterm_types::pty::PendingPrompt {
        question: "opaque-question-62".into(),
        options: vec![
            codeterm_types::pty::PromptOption {
                label: "opaque-choice-62".into(),
                value: "1".into(),
            },
            codeterm_types::pty::PromptOption {
                label: "opaque-choice-63".into(),
                value: "2".into(),
            },
        ],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    }
}

#[test]
fn cron_reap_requires_a_seen_then_absent_provider_tree() {
    use crate::service::process_tree::{
        ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeKey, ProviderTreeWitnesses,
    };

    let key = ProviderTreeKey {
        tab_id: "cron-tab".into(),
        pty_id: "cron-pty".into(),
        binding_nonce: Some("cron-nonce".into()),
        executable_name: "provider-binary".into(),
    };
    let witnesses = ProviderTreeWitnesses::default();
    let present = [ProcessIdentity {
        pid: 91,
        start_token: "process-91".into(),
    }]
    .into_iter()
    .collect();
    witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Present(present));
    let exited = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let unseen =
        ProviderTreeWitnesses::default().record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let cron = AgentRunKind::Cron {
        job_id: "job".into(),
        run_id: "run".into(),
    };
    let memory = AgentRunKind::MemWorker {
        worker_id: "worker".into(),
    };

    assert!(cron_provider_exit_is_proven(&cron, Some(&exited)));
    assert!(!cron_provider_exit_is_proven(&cron, Some(&unseen)));
    assert!(!cron_provider_exit_is_proven(&memory, Some(&exited)));
    assert!(!cron_provider_exit_is_proven(&cron, None));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn idle_poll_stores_provider_prompt_without_notification() {
    let (fixture, tab_id, pty_id) = owner_prompt_fixture();

    assert!(surface_idle_provider_prompt(
        &fixture.service,
        &tab_id,
        &pty_id
    ));
    assert!(surface_idle_provider_prompt(
        &fixture.service,
        &tab_id,
        &pty_id
    ));

    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).expect("prompt remains on tab");
    assert!(tab.pending_prompt.is_some());
    assert_eq!(
        tab.pending_prompt_kind,
        Some(codeterm_types::pty::PromptKind::Provider)
    );
    assert_eq!(
        tab.boot_stage,
        Some(crate::service::boot_stage::BootStage::AwaitingOwnerPrompt)
    );
    drop(mux);

    assert!(fixture.service.notif.lock().list(false).is_empty());

    fixture
        .service
        .close_tab_with_reason(
            &tab_id,
            crate::multiplexer::types::TabCloseReason::AgentRunEnded,
        )
        .unwrap();
    assert!(fixture.service.notif.lock().list(false).is_empty());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn dead_cron_run_is_reaped_on_process_exit() {
    use tauri::Manager as _;
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&tab_id).unwrap();
    let pty_id = "cron-process-exit-pty".to_string();
    mux.link_pty(&tab_id, pty_id.clone());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let cron = std::sync::Arc::new(parking_lot::Mutex::new(
        crate::cron::store::CronStore::new_with_path(fixture.temp.path().join("crons.json")),
    ));
    fixture._app.manage(cron.clone());
    fixture
        ._app
        .manage::<crate::CronWake>(std::sync::Arc::new(std::sync::Condvar::new()));
    let run_id = "process-exit-run";
    let job_id = {
        let mut store = cron.lock();
        let job = store
            .create(crate::cron::store::CronCreateParams {
                name: "process-exit".into(),
                workspace_id,
                team_id: None,
                detach_from_team: false,
                schedule: "in 1h".into(),
                task: "run once".into(),
                description: None,
                agent: None,
                model: None,
                reasoning_effort: None,
                cwd: None,
                role: None,
                skill: None,
                creator_tab_id: None,
                report_target_tab_id: None,
                enabled: Some(true),
                max_history: None,
                oneshot: Some(true),
                jitter_max_ms: Some(0),
                tz: None,
                timeout_seconds: None,
            })
            .unwrap();
        store
            .record_run(
                &job.id,
                crate::cron::store::CronRunInfo {
                    run_id: run_id.into(),
                    started_at: 1,
                    finished_at: None,
                    status: crate::cron::store::CronRunStatus::Running,
                    tab_id: Some(tab_id.clone()),
                    session_id: None,
                    cwd: None,
                    summary: None,
                    report_delivery: None,
                },
            )
            .unwrap();
        store.register_tab(&tab_id, &job.id, run_id);
        job.id
    };
    let run = std::sync::Arc::new(RunState {
        pty_id: pty_id.clone(),
        tab_id,
        kind: AgentRunKind::Cron {
            job_id: job_id.clone(),
            run_id: run_id.into(),
        },
        scanner: parking_lot::Mutex::new(ResultEventScanner::new()),
        started_at: std::time::Instant::now(),
        last_byte_at: parking_lot::Mutex::new(std::time::Instant::now()),
        completed: AtomicBool::new(false),
        armed: AtomicBool::new(false),
        saw_working: AtomicBool::new(false),
        saw_post_session_progress: AtomicBool::new(false),
        restart_count: AtomicU32::new(0),
        last_restart_at: parking_lot::Mutex::new(std::time::Instant::now()),
        resume_system_prompt: None,
    });
    fixture
        .service
        .run_watcher
        .runs
        .lock()
        .insert(pty_id.clone(), run.clone());

    assert!(fixture.service.run_watcher.fail_cron_on_process_exit(
        &fixture.service,
        &pty_id,
        "process-exit-token".into(),
    ));

    assert!(run.completed.load(Ordering::Acquire));
    assert!(!fixture
        .service
        .run_watcher
        .runs
        .lock()
        .contains_key(&pty_id));
    let store = cron.lock();
    let history = store.get_history(&job_id);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].status, crate::cron::store::CronRunStatus::Failed);
    assert!(store
        .read_log(&job_id, run_id)
        .expect("process-exit log")
        .contains("process-exit-token"));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn dream_deadline_hook_finalizes_diary_on_hard_timeout() {
    use codeterm_mem::dream_status::deadline::{DEADLINE_REASON, DREAM_DIARY_TAG};
    use codeterm_mem::dream_status::{diary_evidence, DreamRunStatus, ExecutionEnd};

    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = "dream-deadline-pty".to_string();
    mux.link_pty(&tab_id, pty_id.clone());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    let worker_id = format!("gw_test_{}", uuid::Uuid::new_v4().simple());
    svc.workers.register(crate::mem::workers::make_info(
        worker_id.clone(),
        tab_id.clone(),
        "dreaming:rem".into(),
        "system:dreaming".into(),
        "hsl(0,0%,0%)".into(),
    ));
    let diary = svc
        .mem
        .save(crate::mem::NewMemItem {
            kind: crate::mem::MemKind::Daily,
            scope: crate::mem::MemScope::Global,
            title: "dream: deadline hook".into(),
            summary: Some("WIP: dreaming".into()),
            area: Some("_dreams".into()),
            tags: vec![DREAM_DIARY_TAG.into()],
            body: "<insight></insight><log>STARTED: t0\nHUBS_BEFORE: 3\nHUBS: pending\n</log>"
                .into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    let run = RunState {
        pty_id: pty_id.clone(),
        tab_id,
        kind: AgentRunKind::MemWorker {
            worker_id: worker_id.clone(),
        },
        scanner: parking_lot::Mutex::new(ResultEventScanner::new()),
        started_at: std::time::Instant::now(),
        last_byte_at: parking_lot::Mutex::new(std::time::Instant::now()),
        completed: AtomicBool::new(true),
        armed: AtomicBool::new(false),
        saw_working: AtomicBool::new(false),
        saw_post_session_progress: AtomicBool::new(false),
        restart_count: AtomicU32::new(0),
        last_restart_at: parking_lot::Mutex::new(std::time::Instant::now()),
        resume_system_prompt: None,
    };

    end_on_hard_timeout(svc, &run, 1800);

    let worker = svc.workers.get(&worker_id).unwrap();
    if let Some(log_path) = worker.log_path.as_deref() {
        let _ = std::fs::remove_file(log_path);
    }
    assert!(matches!(
        worker.execution_end,
        Some(ExecutionEnd::TimedOut { .. })
    ));
    assert_eq!(
        worker.dream_run_status(),
        DreamRunStatus::PartiallyFinalized {
            reason: DEADLINE_REASON.into()
        }
    );
    assert_eq!(worker.diary_id.as_deref(), Some(diary.as_str()));
    let stored = svc.mem.get(&diary).unwrap().unwrap();
    let evidence = diary_evidence(&diary, &stored.body);
    assert!(
        evidence.is_complete() && evidence.cut_at_deadline,
        "{evidence:?}"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires desktop runtime")]
fn run_watcher_mem_worker_ends_on_pending_provider_prompt() {
    use codeterm_mem::dream_status::deadline::{APPROVAL_PROMPT_REASON, DREAM_DIARY_TAG};
    use codeterm_mem::dream_status::{
        diary_evidence, DiaryFinalization, DreamRunStatus, ExecutionEnd,
    };
    use codeterm_types::pty::PromptKind;

    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = format!("approval-prompt-{}", uuid::Uuid::new_v4().simple());
    mux.link_pty(&tab_id, pty_id.clone());
    let worker_id = format!("gw_test_{}", uuid::Uuid::new_v4().simple());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.hidden = true;
    tab.worker_id = Some(worker_id.clone());
    tab.pending_prompt = Some(owner_prompt());
    tab.pending_prompt_kind = Some(PromptKind::Provider);

    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let svc = &fixture.service;
    svc.workers.register(crate::mem::workers::make_info(
        worker_id.clone(),
        tab_id.clone(),
        "dreaming:rem".into(),
        "system:dreaming".into(),
        "hsl(0,0%,0%)".into(),
    ));
    let diary = svc
        .mem
        .save(crate::mem::NewMemItem {
            kind: crate::mem::MemKind::Daily,
            scope: crate::mem::MemScope::Global,
            title: "dream: approval prompt".into(),
            summary: Some("WIP: dreaming".into()),
            area: Some("_dreams".into()),
            tags: vec![DREAM_DIARY_TAG.into()],
            body: "<insight></insight><log>STARTED: t0\nHUBS_BEFORE: 3\nHUBS: pending\n</log>"
                .into(),
            source: None,
            importance: 3.0,
            source_path: None,
            pinned: false,
        })
        .unwrap();
    let run = Arc::new(RunState {
        pty_id: pty_id.clone(),
        tab_id: tab_id.clone(),
        kind: AgentRunKind::MemWorker {
            worker_id: worker_id.clone(),
        },
        scanner: Mutex::new(ResultEventScanner::new()),
        started_at: Instant::now(),
        last_byte_at: Mutex::new(Instant::now() - Duration::from_secs(IDLE_COMPLETE_SECS + 1)),
        completed: AtomicBool::new(false),
        armed: AtomicBool::new(false),
        saw_working: AtomicBool::new(false),
        saw_post_session_progress: AtomicBool::new(false),
        restart_count: AtomicU32::new(0),
        last_restart_at: Mutex::new(Instant::now()),
        resume_system_prompt: None,
    });
    svc.run_watcher
        .runs
        .lock()
        .insert(pty_id.clone(), run.clone());

    let completed = tauri::async_runtime::block_on(tokio::time::timeout(
        Duration::from_secs(3),
        watchdog_loop(svc.clone(), run.clone(), 3600),
    ));
    assert!(
        completed.is_ok(),
        "blocking prompt end should run on the first tick"
    );
    assert!(run.completed.load(Ordering::Acquire));

    let worker = svc.workers.get(&worker_id).unwrap();
    assert_eq!(
        worker.execution_end,
        Some(ExecutionEnd::TimedOut {
            detail: APPROVAL_PROMPT_REASON.into(),
        })
    );
    assert_eq!(
        worker.finalization,
        Some(DiaryFinalization::Partial {
            diary_id: Some(diary.clone()),
            reason: APPROVAL_PROMPT_REASON.into(),
        })
    );
    assert_eq!(
        worker.dream_run_status(),
        DreamRunStatus::PartiallyFinalized {
            reason: APPROVAL_PROMPT_REASON.into(),
        }
    );
    let stored = svc.mem.get(&diary).unwrap().unwrap();
    let evidence = diary_evidence(&diary, &stored.body);
    assert!(evidence.is_complete() && evidence.cut_at_deadline);
    assert!(!svc.run_watcher.runs.lock().contains_key(&pty_id));
}

#[test]
fn run_watcher_mem_worker_busy_without_prompt_is_untouched() {
    let kind = AgentRunKind::MemWorker {
        worker_id: "worker".into(),
    };
    assert!(!should_end_on_blocking_prompt(
        &kind,
        false,
        Some(codeterm_types::pty::PromptKind::Provider)
    ));
    assert!(!should_end_on_blocking_prompt(&kind, false, None));
}

#[test]
fn run_watcher_mem_worker_ends_on_pending_structured_prompt() {
    let kind = AgentRunKind::MemWorker {
        worker_id: "worker".into(),
    };
    assert!(should_end_on_blocking_prompt(
        &kind,
        true,
        Some(codeterm_types::pty::PromptKind::Structured)
    ));
}

#[test]
fn run_watcher_mem_worker_directory_trust_prompt_is_untouched() {
    let kind = AgentRunKind::MemWorker {
        worker_id: "worker".into(),
    };
    assert!(!should_end_on_blocking_prompt(
        &kind,
        true,
        Some(codeterm_types::pty::PromptKind::DirectoryTrust)
    ));
    assert!(!should_end_on_blocking_prompt(
        &kind,
        true,
        Some(codeterm_types::pty::PromptKind::ManualUpdate)
    ));
}

#[test]
fn run_watcher_non_mem_worker_with_prompt_is_untouched() {
    let kind = AgentRunKind::Cron {
        job_id: "job".into(),
        run_id: "run".into(),
    };
    assert!(!should_end_on_blocking_prompt(
        &kind,
        true,
        Some(codeterm_types::pty::PromptKind::Provider)
    ));
    assert!(!should_end_on_blocking_prompt(
        &kind,
        true,
        Some(codeterm_types::pty::PromptKind::Structured)
    ));
}

#[test]
fn run_watcher_blocking_prompt_end_is_idempotent_with_hard_timeout() {
    let run = RunState {
        pty_id: "approval-prompt-pty".into(),
        tab_id: "approval-prompt-tab".into(),
        kind: AgentRunKind::MemWorker {
            worker_id: "worker".into(),
        },
        scanner: Mutex::new(ResultEventScanner::new()),
        started_at: Instant::now(),
        last_byte_at: Mutex::new(Instant::now()),
        completed: AtomicBool::new(false),
        armed: AtomicBool::new(false),
        saw_working: AtomicBool::new(false),
        saw_post_session_progress: AtomicBool::new(false),
        restart_count: AtomicU32::new(0),
        last_restart_at: Mutex::new(Instant::now()),
        resume_system_prompt: None,
    };

    assert!(claim_run_end(&run));
    assert!(!claim_run_end(&run));
}
