use super::*;
use parking_lot::Mutex;

#[derive(Default)]
struct MockSink {
    events: Mutex<Vec<(String, BootStage, Option<String>)>>,
}

impl BootStageSink for MockSink {
    fn record(&self, tab_id: &str, stage: BootStage, reason: Option<&str>) {
        self.events
            .lock()
            .push((tab_id.to_string(), stage, reason.map(str::to_string)));
    }
}

impl MockSink {
    fn stages(&self) -> Vec<BootStage> {
        self.events.lock().iter().map(|(_, s, _)| *s).collect()
    }
}

#[test]
fn happy_path_emits_stages_in_order() {
    let sink = MockSink::default();
    for stage in BootStage::HAPPY_PATH {
        sink.record("tab-1", stage, None);
    }
    assert_eq!(sink.stages(), BootStage::HAPPY_PATH.to_vec());
    assert!(sink
        .events
        .lock()
        .iter()
        .all(|(pid, _, r)| pid == "tab-1" && r.is_none()));
}

#[test]
fn failure_path_ends_at_spawn_failed_with_reason_and_no_ready() {
    let sink = MockSink::default();
    sink.record("tab-2", BootStage::PtySpawned, None);
    sink.record("tab-2", BootStage::ShellReady, None);
    sink.record(
        "tab-2",
        BootStage::SpawnFailed,
        Some("launch_cmd failed: boom"),
    );

    let stages = sink.stages();
    assert!(!stages.contains(&BootStage::AgentReady));
    assert_eq!(stages.last(), Some(&BootStage::SpawnFailed));
    let (_, last_stage, last_reason) = sink.events.lock().last().cloned().unwrap();
    assert_eq!(last_stage, BootStage::SpawnFailed);
    assert_eq!(last_reason.as_deref(), Some("launch_cmd failed: boom"));
}

/// An options block surfaced exactly as the live path surfaces it: the
/// structural detector on the tab's own attributed grid. No vendor wording.
fn blocking_prompt_from_screen() -> codeterm_types::pty::PendingPrompt {
    let mut parser = vt100::Parser::new(24, 80, 0);
    for line in [
        "Select an option",
        "",
        "\u{276f} 1. first choice",
        "  2. second choice",
        "  3. third choice",
        "",
        "  enter to select \u{b7} esc to cancel",
    ] {
        parser.process(line.as_bytes());
        parser.process(b"\r\n");
    }
    let input = crate::agent::native_prompt::NativePromptInput {
        screen: codeterm_pty::attributed_screen::AttributedScreen::from_vt100(parser.screen()),
        cursor_hidden: true,
        alt_screen: false,
    };
    crate::agent::native_prompt::pending_prompt_from_input(&input)
        .expect("mechanism probe: the structural detector surfaces this options block")
}

fn tab_with_prompt(prompt: Option<codeterm_types::pty::PendingPrompt>) -> Tab {
    let (mut state, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&workspace_id).unwrap();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.pending_prompt = prompt;
    tab.clone()
}

#[test]
fn inferred_prompt_never_withholds_agent_ready() {
    let mut tab = tab_with_prompt(Some(blocking_prompt_from_screen()));
    apply_boot_stage(&mut tab, BootStage::AgentReady, None);
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
}

/// The over-correction guard: a fix that never reports ready also never
/// reports ready while blocked, and is just as wrong.
#[test]
fn agent_ready_is_recorded_when_nothing_owns_the_input() {
    let mut tab = tab_with_prompt(None);
    apply_boot_stage(&mut tab, BootStage::AgentReady, None);
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
}

const EVERY_STAGE: [BootStage; 14] = [
    BootStage::PtyStarting,
    BootStage::PtySpawned,
    BootStage::ReaderReady,
    BootStage::ShellReady,
    BootStage::AgentLaunching,
    BootStage::LaunchWritten,
    BootStage::ProviderReady,
    BootStage::PreparingRoleInstructions,
    BootStage::DeliveringInstructions,
    BootStage::AgentReady,
    BootStage::PtyReattached,
    BootStage::LaunchDeadlineElapsed,
    BootStage::LaunchBindingUnconfirmed,
    BootStage::SpawnFailed,
];

/// Keeps the cross-product below honest: a new variant fails to compile
/// here until `EVERY_STAGE` learns about it.
#[test]
fn every_stage_covers_the_enum() {
    fn index(stage: BootStage) -> usize {
        match stage {
            BootStage::PtyStarting => 0,
            BootStage::PtySpawned => 1,
            BootStage::ReaderReady => 2,
            BootStage::ShellReady => 3,
            BootStage::AgentLaunching => 4,
            BootStage::LaunchWritten => 5,
            BootStage::ProviderReady => 6,
            BootStage::PreparingRoleInstructions => 7,
            BootStage::DeliveringInstructions => 8,
            BootStage::AgentReady => 9,
            BootStage::PtyReattached => 10,
            BootStage::LaunchDeadlineElapsed => 11,
            BootStage::LaunchBindingUnconfirmed => 12,
            BootStage::SpawnFailed => 13,
        }
    }
    let mut seen = vec![false; EVERY_STAGE.len()];
    for stage in EVERY_STAGE {
        seen[index(stage)] = true;
    }
    assert!(seen.into_iter().all(|covered| covered));
}

fn authenticated_tab(current: Option<BootStage>) -> Tab {
    let mut tab = tab_with_prompt(None);
    tab.boot_stage = current;
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some("01a090f2".into());
    tab.session_binding_nonce = Some("nonce-a".into());
    tab.reported_session_binding_nonce = Some("nonce-a".into());
    tab
}

#[test]
fn launch_nonce_is_proven_only_when_this_launch_echoed_it() {
    assert!(authenticated_tab(Some(BootStage::ProviderReady)).launch_session_authenticated());

    let mut unreported = authenticated_tab(Some(BootStage::ProviderReady));
    unreported.reported_session_binding_nonce = None;
    assert!(!unreported.launch_session_authenticated());

    let mut superseded = authenticated_tab(Some(BootStage::ProviderReady));
    superseded.session_binding_nonce = Some("nonce-b".into());
    assert!(!superseded.launch_session_authenticated());

    let mut nonceless = authenticated_tab(Some(BootStage::ProviderReady));
    nonceless.session_binding_nonce = None;
    nonceless.reported_session_binding_nonce = None;
    assert!(!nonceless.launch_session_authenticated());

    let mut unbound = authenticated_tab(Some(BootStage::ProviderReady));
    unbound.agent_session_id = None;
    assert!(!unbound.launch_session_authenticated());
}

/// The contradiction the daemon used to report: a live, nonce-verified
/// agent behind a tab whose boot stage called the spawn failed.
#[test]
fn spawn_failure_is_refused_once_the_agent_proved_this_launch() {
    for current in [BootStage::ProviderReady, BootStage::AgentReady] {
        assert_eq!(
            honest_boot_stage(BootStage::SpawnFailed, Some(current), true),
            BootStage::AgentReady,
        );
    }
}

#[test]
fn spawn_failure_stands_without_a_binding_proof() {
    for current in EVERY_STAGE {
        assert_eq!(
            honest_boot_stage(BootStage::SpawnFailed, Some(current), false),
            BootStage::SpawnFailed,
        );
    }
    assert_eq!(
        honest_boot_stage(BootStage::SpawnFailed, None, false),
        BootStage::SpawnFailed,
    );
}

/// The over-correction guard: a proof alone never excuses a launch that
/// never got the provider up, so a real early failure still latches.
#[test]
fn spawn_failure_stands_before_the_provider_is_up() {
    let short_of_provider = EVERY_STAGE
        .into_iter()
        .map(Some)
        .chain(std::iter::once(None))
        .filter(|stage| {
            !matches!(
                stage,
                Some(BootStage::ProviderReady) | Some(BootStage::AgentReady)
            )
        });
    for current in short_of_provider {
        assert_eq!(
            honest_boot_stage(BootStage::SpawnFailed, current, true),
            BootStage::SpawnFailed,
        );
    }
}

#[test]
fn no_other_stage_is_ever_rewritten() {
    for stage in EVERY_STAGE
        .into_iter()
        .filter(|s| *s != BootStage::SpawnFailed)
    {
        for current in EVERY_STAGE.into_iter().map(Some).chain([None]) {
            for proven in [false, true] {
                assert_eq!(honest_boot_stage(stage, current, proven), stage);
            }
        }
    }
}

/// The invariant itself, over every reachable combination: a recorded
/// spawn failure and a proven live launch cannot coexist on one tab.
#[test]
fn a_recorded_spawn_failure_never_coexists_with_a_proven_live_launch() {
    for current in EVERY_STAGE.into_iter().map(Some).chain([None]) {
        for stage in EVERY_STAGE {
            let mut tab = authenticated_tab(current);
            let honest =
                honest_boot_stage(stage, tab.boot_stage, tab.launch_session_authenticated());
            apply_boot_stage(&mut tab, honest, Some("boom"));
            let provider_was_up = matches!(
                current,
                Some(BootStage::ProviderReady) | Some(BootStage::AgentReady)
            );
            assert!(
                !(tab.boot_stage == Some(BootStage::SpawnFailed) && provider_was_up),
                "spawn_failed recorded from {current:?} on a tab whose agent proved this launch",
            );
        }
    }
}

#[test]
fn a_late_binding_disproves_the_failure_it_arrives_after() {
    let mut failed = authenticated_tab(Some(BootStage::SpawnFailed));
    assert!(binding_disproves_latched_failure(&failed));

    failed.reported_session_binding_nonce = None;
    assert!(
        !binding_disproves_latched_failure(&failed),
        "an unproven launch keeps its failure",
    );

    let ready = authenticated_tab(Some(BootStage::AgentReady));
    assert!(!binding_disproves_latched_failure(&ready));
}

fn binding(pty_id: &str, nonce: &str) -> crate::service::ProviderSessionBinding {
    crate::service::ProviderSessionBinding {
        pty_id: pty_id.to_string(),
        binding_nonce: nonce.to_string(),
    }
}

const DEADLINE_REASON: &str = "agent output deadline elapsed";

/// A launch that wrote its command and then produced nothing observable.
fn launching_tab(pty_id: &str, nonce: &str) -> Tab {
    let mut tab = tab_with_prompt(None);
    tab.pty_id = Some(pty_id.into());
    tab.agent_type = Some("codex".into());
    tab.session_binding_nonce = Some(nonce.into());
    tab.reported_session_binding_nonce = None;
    tab.agent_session_id = None;
    tab.boot_stage = Some(BootStage::ShellReady);
    tab
}

/// Mirrors `record_provisional_launch`: the latch rule gates the write, and
/// the stage lands together with the evidence that produced it.
fn latch_provisional(tab: &mut Tab, pty_id: &str, stage: BootStage, reason: &str) {
    let binding_nonce = tab
        .session_binding_nonce
        .clone()
        .expect("a current launch has a binding nonce");
    assert!(apply_provisional_launch(
        tab,
        pty_id,
        &binding_nonce,
        stage,
        reason,
    ));
}

fn latch_deadline(tab: &mut Tab, pty_id: &str) {
    latch_provisional(
        tab,
        pty_id,
        BootStage::LaunchDeadlineElapsed,
        DEADLINE_REASON,
    );
}

/// The evidence the first-run producer carries when the payload reached the
/// PTY and the authoritative binding never arrived.
const UNCONFIRMED_REASON: &str = "submit_unconfirmed: launch payload written, pending evidence \
         retained; authoritative provider session binding was not reported";

fn latch_submit_unconfirmed(tab: &mut Tab, pty_id: &str) {
    tab.boot_stage = Some(BootStage::ProviderReady);
    latch_provisional(
        tab,
        pty_id,
        BootStage::LaunchBindingUnconfirmed,
        UNCONFIRMED_REASON,
    );
}

/// The agent naming itself on the latched launch: its own accepted session
/// plus this attempt's nonce echoed back.
fn report_own_launch(tab: &mut Tab, nonce: &str) {
    tab.agent_session_id = Some("01a090f2".into());
    tab.reported_session_binding_nonce = Some(nonce.into());
}

#[test]
fn deadline_failure_is_provisional_until_current_attempt_late_readiness() {
    let mut tab = launching_tab("pty-a", "nonce-a");
    latch_deadline(&mut tab, "pty-a");

    assert_eq!(tab.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
    assert_ne!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert!(tab.boot_stage.is_some_and(|stage| stage.is_provisional()));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some(DEADLINE_REASON));
    assert_eq!(
        tab.provisional_launch
            .as_ref()
            .map(|latch| latch.reason.as_str()),
        Some(DEADLINE_REASON),
        "the deadline stays diagnosable while it is provisional",
    );
    assert!(
        !late_readiness_retires_provisional_launch(&tab, &binding("pty-a", "nonce-a")),
        "an unechoed nonce is not the agent naming itself",
    );

    report_own_launch(&mut tab, "nonce-a");
    assert!(late_readiness_retires_provisional_launch(
        &tab,
        &binding("pty-a", "nonce-a")
    ));

    apply_boot_stage(&mut tab, BootStage::AgentReady, None);
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert_eq!(tab.provisional_launch, None);
}

#[test]
fn late_agent_ready_wins_over_provisional_deadline_stage() {
    let mut tab = launching_tab("pty-a", "nonce-a");
    assert!(apply_provisional_launch(
        &mut tab,
        "pty-a",
        "nonce-a",
        BootStage::LaunchDeadlineElapsed,
        DEADLINE_REASON,
    ));
    report_own_launch(&mut tab, "nonce-a");

    assert!(apply_late_readiness(&mut tab, &binding("pty-a", "nonce-a")));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert_eq!(tab.provisional_launch, None);
}

#[test]
fn provisional_deadline_never_overwrites_recorded_agent_ready() {
    let mut tab = authenticated_tab(Some(BootStage::AgentReady));
    tab.pty_id = Some("pty-a".into());

    assert!(!apply_provisional_launch(
        &mut tab,
        "pty-a",
        "nonce-a",
        BootStage::LaunchDeadlineElapsed,
        DEADLINE_REASON,
    ));
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.provisional_launch, None);
}

#[test]
fn concurrent_ready_and_deadline_commits_keep_generation_evidence_atomic() {
    use std::sync::{Arc, Barrier};

    let (mut state, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace_id = state.list_workspaces()[0].id.clone();
    let tab_id = state.create_tab(&workspace_id).unwrap();
    {
        let tab = state.get_tab_mut(&tab_id).unwrap();
        tab.pty_id = Some("pty-a".into());
        tab.agent_type = Some("codex".into());
        tab.agent_session_id = Some("01a090f2".into());
        tab.session_binding_nonce = Some("nonce-a".into());
        tab.reported_session_binding_nonce = Some("nonce-a".into());
        tab.boot_stage = Some(BootStage::ShellReady);
    }
    let mux: crate::MuxState = Arc::new(crate::lock_hold::TimedRwLock::new("mux", state));
    let attempt = crate::service::launch_readiness::LaunchAttempt {
        tab_id: tab_id.clone(),
        pty_id: "pty-a".into(),
        binding_nonce: "nonce-a".into(),
    };
    let evidence = binding(&attempt.pty_id, &attempt.binding_nonce);
    let barrier = Arc::new(Barrier::new(3));
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));

    let deadline_mux = mux.clone();
    let deadline_tab_id = tab_id.clone();
    let deadline_attempt = attempt.clone();
    let deadline_barrier = barrier.clone();
    let deadline_events = events.clone();
    let deadline = std::thread::spawn(move || {
        deadline_barrier.wait();
        commit_boot_stage_transition(
            &deadline_mux,
            &deadline_tab_id,
            |tab| {
                apply_provisional_launch(
                    tab,
                    &deadline_attempt.pty_id,
                    &deadline_attempt.binding_nonce,
                    BootStage::LaunchDeadlineElapsed,
                    DEADLINE_REASON,
                )
            },
            || {
                deadline_events
                    .lock()
                    .push(BootStage::LaunchDeadlineElapsed)
            },
        )
    });

    let ready_mux = mux.clone();
    let ready_tab_id = tab_id.clone();
    let ready_attempt = attempt.clone();
    let ready_evidence = evidence.clone();
    let ready_barrier = barrier.clone();
    let ready_events = events.clone();
    let ready = std::thread::spawn(move || {
        ready_barrier.wait();
        commit_boot_stage_transition(
            &ready_mux,
            &ready_tab_id,
            |tab| {
                let owns_evidence = ready_evidence.pty_id == ready_attempt.pty_id
                    && ready_evidence.binding_nonce == ready_attempt.binding_nonce
                    && tab.pty_id.as_deref() == Some(ready_evidence.pty_id.as_str())
                    && tab.session_binding_nonce.as_deref()
                        == Some(ready_evidence.binding_nonce.as_str())
                    && tab.launch_session_authenticated();
                if !owns_evidence {
                    return false;
                }
                if tab.boot_stage.is_some_and(BootStage::is_provisional) {
                    return apply_late_readiness(tab, &ready_evidence);
                }
                apply_boot_stage(tab, BootStage::AgentReady, None);
                true
            },
            || ready_events.lock().push(BootStage::AgentReady),
        )
    });

    barrier.wait();
    deadline.join().unwrap();
    ready.join().unwrap();

    let state = mux.read();
    let tab = state.get_tab(&tab_id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.provisional_launch, None);
    assert_eq!(events.lock().last(), Some(&BootStage::AgentReady));
}

#[test]
fn stale_generation_cannot_latch_a_deadline_on_the_same_pty() {
    let mut current = launching_tab("pty-a", "nonce-current");

    assert!(!apply_provisional_launch(
        &mut current,
        "pty-a",
        "nonce-previous",
        BootStage::LaunchDeadlineElapsed,
        DEADLINE_REASON,
    ));
    assert_eq!(current.boot_stage, Some(BootStage::ShellReady));
    assert_eq!(current.provisional_launch, None);
}

/// The retirement reads the launch record, never the grid: a PTY that never
/// paints another cell after the deadline still recovers.
#[test]
fn late_readiness_without_screen_update_reconciles_current_attempt() {
    let screen_state = codeterm_util::lock_hold::TimedMutex::new(
        "screen_state",
        codeterm_pty::pty::screen_state::ScreenState::new(),
    );
    let signal = screen_state.lock().arm_screen_change_signal("pty-a");
    let quiet = signal.generation();

    let mut tab = launching_tab("pty-a", "nonce-a");
    latch_deadline(&mut tab, "pty-a");
    report_own_launch(&mut tab, "nonce-a");

    assert!(late_readiness_retires_provisional_launch(
        &tab,
        &binding("pty-a", "nonce-a")
    ));
    apply_boot_stage(&mut tab, BootStage::AgentReady, None);

    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(
        signal.generation(),
        quiet,
        "recovery consumed no screen update",
    );
    assert!(!signal.wait_after(quiet, std::time::Duration::ZERO));
}

/// Mirrors `retire_provisional_launch`: the rule is the only gate on
/// the write, so evidence it rejects leaves the latched launch untouched.
fn offer_late_readiness(tab: &mut Tab, evidence: &crate::service::ProviderSessionBinding) {
    if late_readiness_retires_provisional_launch(tab, evidence) {
        apply_boot_stage(tab, BootStage::AgentReady, None);
    }
}

#[test]
fn stale_or_invalid_evidence_cannot_unlock_deadline_failure() {
    // A prior PTY, a superseded attempt, and both at once.
    let mut latched = launching_tab("pty-a", "nonce-a");
    latch_deadline(&mut latched, "pty-a");
    report_own_launch(&mut latched, "nonce-a");
    let latch = latched.provisional_launch.clone();
    for evidence in [
        binding("pty-old", "nonce-a"),
        binding("pty-a", "nonce-old"),
        binding("pty-old", "nonce-old"),
    ] {
        offer_late_readiness(&mut latched, &evidence);
    }
    assert_eq!(latched.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
    assert_eq!(latched.provisional_launch, latch);
    assert_eq!(
        latched.boot_failure_reason.as_deref(),
        Some(DEADLINE_REASON)
    );

    // A bare session id, with no nonce echoed at all, never unlocks input.
    let mut bare_session = launching_tab("pty-a", "nonce-a");
    latch_deadline(&mut bare_session, "pty-a");
    bare_session.agent_session_id = Some("01a090f2".into());
    offer_late_readiness(&mut bare_session, &binding("pty-a", "nonce-a"));
    assert_eq!(
        bare_session.boot_stage,
        Some(BootStage::LaunchDeadlineElapsed)
    );

    // An invalid nonce: the agent echoed something other than this attempt,
    // so neither the attempt's nonce nor the echoed one is authoritative.
    let mut invalid = launching_tab("pty-a", "nonce-a");
    latch_deadline(&mut invalid, "pty-a");
    report_own_launch(&mut invalid, "nonce-b");
    for evidence in [binding("pty-a", "nonce-a"), binding("pty-a", "nonce-b")] {
        offer_late_readiness(&mut invalid, &evidence);
    }
    assert_eq!(invalid.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
    assert_eq!(
        invalid.session_binding_nonce.as_deref(),
        Some("nonce-a"),
        "rejected evidence never overwrites the current attempt",
    );

    // The tab relinked to a new PTY after the deadline: the latched launch
    // is no longer current, so nothing can speak for it.
    let mut relinked = launching_tab("pty-a", "nonce-a");
    latch_deadline(&mut relinked, "pty-a");
    report_own_launch(&mut relinked, "nonce-a");
    relinked.pty_id = Some("pty-b".into());
    for evidence in [binding("pty-a", "nonce-a"), binding("pty-b", "nonce-a")] {
        offer_late_readiness(&mut relinked, &evidence);
    }
    assert_eq!(relinked.boot_stage, Some(BootStage::LaunchDeadlineElapsed));
}

#[test]
fn genuine_failed_launch_remains_blocked() {
    let mut failed = launching_tab("pty-a", "nonce-a");
    apply_boot_stage(
        &mut failed,
        BootStage::SpawnFailed,
        Some("PtySpawnFailed: boom"),
    );
    assert_eq!(failed.boot_stage, Some(BootStage::SpawnFailed));
    assert_eq!(
        failed.boot_failure_reason.as_deref(),
        Some("PtySpawnFailed: boom")
    );
    assert_eq!(failed.provisional_launch, None);

    report_own_launch(&mut failed, "nonce-a");
    assert!(
        !late_readiness_retires_provisional_launch(&failed, &binding("pty-a", "nonce-a")),
        "a genuine failure is not a deadline and is not retired by this rule",
    );
    assert_eq!(
        honest_boot_stage(BootStage::SpawnFailed, Some(BootStage::ShellReady), true),
        BootStage::SpawnFailed,
    );

    // A provisional stage is evidence about one observer, not a shield, so a
    // later spawn failure still latches from either source.
    let latches: [fn(&mut Tab, &str); 2] = [latch_deadline, latch_submit_unconfirmed];
    for latch in latches {
        let mut latched = launching_tab("pty-a", "nonce-a");
        latch(&mut latched, "pty-a");
        apply_boot_stage(&mut latched, BootStage::SpawnFailed, Some("PtyEof"));
        assert_eq!(latched.boot_stage, Some(BootStage::SpawnFailed));
        assert_eq!(latched.boot_failure_reason.as_deref(), Some("PtyEof"));
        assert_eq!(latched.provisional_launch, None);
        report_own_launch(&mut latched, "nonce-a");
        assert!(!late_readiness_retires_provisional_launch(
            &latched,
            &binding("pty-a", "nonce-a")
        ));
    }

    // A launch that already failed is never latched as provisional either.
    assert!(provisional_launch_for(&failed, "pty-a", "nonce-a", UNCONFIRMED_REASON).is_none());
}

/// The producer refuses to latch a launch the tab no longer owns, so a
/// deadline from a superseded attempt cannot overwrite the current one.
#[test]
fn a_deadline_never_latches_a_launch_the_tab_moved_on_from() {
    let mut relinked = launching_tab("pty-b", "nonce-b");
    assert!(provisional_launch_for(&relinked, "pty-a", "nonce-a", DEADLINE_REASON).is_none());

    for settled in [BootStage::AgentReady, BootStage::SpawnFailed] {
        relinked.boot_stage = Some(settled);
        assert!(provisional_launch_for(&relinked, "pty-b", "nonce-b", DEADLINE_REASON).is_none());
    }

    let mut nonceless = launching_tab("pty-b", "nonce-b");
    nonceless.session_binding_nonce = None;
    assert!(provisional_launch_for(&nonceless, "pty-b", "nonce-b", DEADLINE_REASON).is_none());
}

/// The live #477 gap: a launch whose payload reached the PTY and whose
/// binding never arrived is retired by the same rule as a deadline, and by
/// nothing weaker.
#[test]
fn late_binding_retires_submit_unconfirmed_for_current_attempt() {
    let screen_state = codeterm_util::lock_hold::TimedMutex::new(
        "screen_state",
        codeterm_pty::pty::screen_state::ScreenState::new(),
    );
    let signal = screen_state.lock().arm_screen_change_signal("pty-a");
    let quiet = signal.generation();

    let mut tab = launching_tab("pty-a", "nonce-a");
    latch_submit_unconfirmed(&mut tab, "pty-a");

    assert_eq!(tab.boot_stage, Some(BootStage::LaunchBindingUnconfirmed));
    assert_ne!(tab.boot_stage, Some(BootStage::SpawnFailed));
    assert!(tab.boot_stage.is_some_and(|stage| stage.is_provisional()));
    assert_eq!(tab.boot_failure_reason.as_deref(), Some(UNCONFIRMED_REASON));
    assert_eq!(
        tab.provisional_launch
            .as_ref()
            .map(|latch| (latch.pty_id.as_str(), latch.binding_nonce.as_str())),
        Some(("pty-a", "nonce-a")),
        "the provisional state names the PTY and attempt it describes",
    );
    assert!(
        !late_readiness_retires_provisional_launch(&tab, &binding("pty-a", "nonce-a")),
        "an unechoed nonce is not the agent naming itself",
    );

    report_own_launch(&mut tab, "nonce-a");
    assert!(late_readiness_retires_provisional_launch(
        &tab,
        &binding("pty-a", "nonce-a")
    ));
    apply_boot_stage(&mut tab, BootStage::AgentReady, None);

    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert_eq!(tab.boot_failure_reason, None);
    assert_eq!(tab.provisional_launch, None);
    assert_eq!(
        signal.generation(),
        quiet,
        "retirement consumed no screen update",
    );
    assert!(!signal.wait_after(quiet, std::time::Duration::ZERO));
}

#[test]
fn stale_evidence_cannot_retire_submit_unconfirmed() {
    // A prior PTY, a superseded attempt, and both at once.
    let mut latched = launching_tab("pty-a", "nonce-a");
    latch_submit_unconfirmed(&mut latched, "pty-a");
    report_own_launch(&mut latched, "nonce-a");
    let latch = latched.provisional_launch.clone();
    for evidence in [
        binding("pty-old", "nonce-a"),
        binding("pty-a", "nonce-old"),
        binding("pty-old", "nonce-old"),
    ] {
        offer_late_readiness(&mut latched, &evidence);
    }
    assert_eq!(
        latched.boot_stage,
        Some(BootStage::LaunchBindingUnconfirmed)
    );
    assert_eq!(latched.provisional_launch, latch);
    assert_eq!(
        latched.boot_failure_reason.as_deref(),
        Some(UNCONFIRMED_REASON),
        "rejected evidence never overwrites the retained delivery evidence",
    );

    // A bare session id, with no nonce echoed at all, never unlocks input.
    let mut bare_session = launching_tab("pty-a", "nonce-a");
    latch_submit_unconfirmed(&mut bare_session, "pty-a");
    bare_session.agent_session_id = Some("01a090f2".into());
    offer_late_readiness(&mut bare_session, &binding("pty-a", "nonce-a"));
    assert_eq!(
        bare_session.boot_stage,
        Some(BootStage::LaunchBindingUnconfirmed)
    );

    // An invalid nonce: the agent echoed something other than this attempt.
    let mut invalid = launching_tab("pty-a", "nonce-a");
    latch_submit_unconfirmed(&mut invalid, "pty-a");
    report_own_launch(&mut invalid, "nonce-b");
    for evidence in [binding("pty-a", "nonce-a"), binding("pty-a", "nonce-b")] {
        offer_late_readiness(&mut invalid, &evidence);
    }
    assert_eq!(
        invalid.boot_stage,
        Some(BootStage::LaunchBindingUnconfirmed)
    );
    assert_eq!(
        invalid.session_binding_nonce.as_deref(),
        Some("nonce-a"),
        "rejected evidence never overwrites the current attempt",
    );

    // The tab relinked to a new PTY: the latched launch is no longer
    // current, so nothing can speak for it.
    let mut relinked = launching_tab("pty-a", "nonce-a");
    latch_submit_unconfirmed(&mut relinked, "pty-a");
    report_own_launch(&mut relinked, "nonce-a");
    relinked.pty_id = Some("pty-b".into());
    for evidence in [binding("pty-a", "nonce-a"), binding("pty-b", "nonce-a")] {
        offer_late_readiness(&mut relinked, &evidence);
    }
    assert_eq!(
        relinked.boot_stage,
        Some(BootStage::LaunchBindingUnconfirmed)
    );
}

#[test]
fn stage_serializes_snake_case() {
    let json = serde_json::to_string(&BootStage::AgentLaunching).unwrap();
    assert_eq!(json, "\"agent_launching\"");
    assert_eq!(BootStage::SpawnFailed.as_str(), "spawn_failed");
    assert_eq!(
        serde_json::to_string(&BootStage::LaunchDeadlineElapsed).unwrap(),
        "\"launch_deadline_elapsed\"",
    );
    assert_eq!(
        BootStage::LaunchDeadlineElapsed.as_str(),
        "launch_deadline_elapsed"
    );
    assert_eq!(
        serde_json::to_string(&BootStage::LaunchBindingUnconfirmed).unwrap(),
        "\"launch_binding_unconfirmed\"",
    );
    assert_eq!(
        BootStage::LaunchBindingUnconfirmed.as_str(),
        "launch_binding_unconfirmed"
    );
}
