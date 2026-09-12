//! Focused coverage for the machine-scoped provider/model spend surface and the two
//! attribution gaps it depends on. Every name carries `mission_control_spend` so the
//! bounded runner filter reaches the whole set in one pass.

use codeterm_plan::service_plan::state::mission_control::{
    KpiLedgerState, KpiNotRecordingReason, ModelSpend, ProviderSpend, SpendScope,
};

use crate::agent::kpi::KpiRecorder;
use crate::agent::kpi_ledger::{KpiEventInput, KpiEventKind, KpiLedger, TokenCounts};
use crate::agent::offline_state_classifier::{TabVerdict, TimelineState};
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanStatus, PlannerRef};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

const NOW: u64 = 10_000;

fn token_row(
    provider: &str,
    model: Option<&str>,
    input: u64,
    uncached: u64,
    output: u64,
) -> KpiEventInput {
    KpiEventInput {
        plan_id: None,
        task_id: None,
        tab_id: "tab-x".into(),
        role: None,
        provider: provider.into(),
        model: model.map(str::to_string),
        timestamp: 1,
        kind: KpiEventKind::TokenUsage,
        bytes: input + output,
        tokens: Some(TokenCounts {
            input,
            uncached_input: uncached,
            output,
        }),
        dedup_key: None,
    }
}

fn coordination_row(provider: &str) -> KpiEventInput {
    KpiEventInput {
        kind: KpiEventKind::Send,
        tokens: None,
        bytes: 4_096,
        ..token_row(provider, Some("sonnet"), 0, 0, 0)
    }
}

fn fixture() -> ServiceFixture {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    ServiceFixture::new(mux)
}

fn ledger_on(fixture: &mut ServiceFixture) -> std::sync::Arc<KpiLedger> {
    let ledger =
        std::sync::Arc::new(KpiLedger::open_at(&fixture.temp.path().join("kpi.db")).unwrap());
    fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());
    ledger
}

fn agent_tab(fixture: &ServiceFixture, agent: &str, session_id: &str) -> String {
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("spend".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(agent.into());
    tab.cwd = Some(fixture.temp.path().display().to_string());
    tab.agent_session_id = Some(session_id.into());
    // The unfocused case the ledger actually recorded: no runtime model evidence yet.
    tab.agent_session_model = None;
    tab.role = Some(AgentRole::Worker);
    tab_id
}

fn model_row<'a>(providers: &'a [ProviderSpend], provider: &str) -> &'a [ModelSpend] {
    providers
        .iter()
        .find(|row| row.provider == provider)
        .map(|row| row.models.as_slice())
        .unwrap_or_else(|| panic!("no provider row for {provider}"))
}

/// A tab the model poller has not reached still knows which session it claims, so the
/// provider's own record for that session is the model — never a provider-shaped guess.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_records_the_verified_session_model_for_an_unfocused_tab() {
    let mut fixture = fixture();
    let session = fixture.temp.path().join("rollout.jsonl");
    std::fs::write(
        &session,
        include_str!("../../agent/kpi/usage/fixtures/codex-rollout.jsonl"),
    )
    .unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session_file("codex-mock", session)
            .with_session_model("sess-live", "gpt-5.5-codex"),
    ));

    let bound = agent_tab(&fixture, "codex-mock", "sess-live");
    let unknown = agent_tab(&fixture, "codex-mock", "sess-never-recorded");
    let ledger = ledger_on(&mut fixture);

    fixture.service.request_kpi_usage_ingest(&bound);
    fixture.service.record_kpi_event(&unknown, KpiEventKind::Nudge, 8);
    fixture.service.kpi.drain();

    let expected = crate::service::spawn::normalize_session_model("gpt-5.5-codex");
    let rows = ledger.list_all().unwrap();
    let usage: Vec<_> = rows
        .iter()
        .filter(|row| row.kind == KpiEventKind::TokenUsage)
        .collect();
    assert!(
        !usage.is_empty(),
        "the provider's session log must produce token_usage rows for this to prove anything"
    );
    assert!(
        usage.iter().all(|row| row.model.as_deref() == Some(expected.as_str())),
        "every new usage row carries the session's verified model, got {:?}",
        usage.iter().map(|row| row.model.clone()).collect::<Vec<_>>()
    );
    let unresolved = rows
        .iter()
        .find(|row| row.tab_id == unknown)
        .expect("the second tab recorded its own event");
    assert_eq!(
        unresolved.model, None,
        "a session the provider has no record for stays unassigned instead of borrowing one"
    );
}

/// Rows written before the model was resolvable must stay visible as their own bucket.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_keeps_unresolved_models_as_their_own_visible_bucket() {
    let mut fixture = fixture();
    let ledger = ledger_on(&mut fixture);
    ledger
        .insert_many(&[
            token_row("codex", None, 300, 100, 100),
            token_row("codex", Some("gpt-5.5-codex"), 100, 50, 0),
        ])
        .unwrap();

    let spend = super::mission_control_ext::machine_spend(&fixture.service);
    let models = model_row(&spend.providers, "codex");
    assert_eq!(models.len(), 2, "the unresolved bucket is a row, not a merge");
    let unassigned = models
        .iter()
        .find(|model| model.model.is_none())
        .expect("the historical NULL model survives aggregation");
    assert_eq!(unassigned.turns, 1);
    assert_eq!(unassigned.input_tokens, 300);
    assert_eq!(unassigned.output_tokens, 100);
    assert_eq!(spend.measured_tokens, 500);
    assert_eq!(unassigned.token_share_pct, 80.0);

    let payload = serde_json::to_value(&spend).unwrap();
    let rows = payload["providers"][0]["models"].as_array().unwrap();
    assert!(
        rows.iter().any(|row| row["model"].is_null()),
        "the wire shape keeps the unknown identity null instead of dropping the row"
    );
}

/// An Orchestrator owning open work and a Planner registered on a plan both carry plan
/// context without any task assignment.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_charges_orchestrator_and_planner_tabs_to_their_active_plan() {
    let mut fixture = fixture();
    let orchestrator = agent_tab(&fixture, "mock", "sess-orch");
    let planner = agent_tab(&fixture, "mock", "sess-planner");

    let mut plan = Plan::new(
        "Spend attribution".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(orchestrator.clone()),
            session_id: None,
            title: Some("Planck".into()),
        },
    );
    plan.status = PlanStatus::Active;
    plan.mode = PlanMode::Execution;
    plan.planner = Some(PlannerRef::Tab {
        tab_id: planner.clone(),
        session_id: None,
        title: Some("Pasteur".into()),
    });
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Open work".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert_eq!(
        fixture.service.plan_store.task_binding_for_tab(&orchestrator),
        None,
        "neither tab holds a task assignment; that is the gap under test"
    );
    for tab in [&orchestrator, &planner] {
        assert_eq!(
            fixture
                .service
                .plan_store
                .taskless_plan_context_candidates_for_tab(tab),
            vec![plan.id.clone()],
            "the stored owner/planner binding is what carries context here"
        );
    }

    let ledger = ledger_on(&mut fixture);
    fixture
        .service
        .record_kpi_event(&orchestrator, KpiEventKind::Poll, 16);
    fixture
        .service
        .record_kpi_event(&planner, KpiEventKind::Poll, 16);
    fixture.service.kpi.drain();

    for tab in [&orchestrator, &planner] {
        let row = ledger
            .list_all()
            .unwrap()
            .into_iter()
            .find(|row| &row.tab_id == tab)
            .expect("each tab recorded its event");
        assert_eq!(row.plan_id.as_deref(), Some(plan.id.as_str()));
        assert_eq!(
            row.task_id, None,
            "plan context without a task must not invent a task id"
        );
    }
}

/// The documented rule for the cases that have no single answer.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_leaves_unbound_and_ambiguous_tabs_unassigned() {
    let mut fixture = fixture();
    let unbound = agent_tab(&fixture, "mock", "sess-unbound");
    let ambiguous = agent_tab(&fixture, "mock", "sess-ambiguous");

    for title in ["First plan", "Second plan"] {
        let mut plan = Plan::new(
            title.into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef {
                tab_id: Some("tab-other".into()),
                session_id: None,
                title: None,
            },
        );
        plan.status = PlanStatus::Active;
        plan.planner = Some(PlannerRef::Tab {
            tab_id: ambiguous.clone(),
            session_id: None,
            title: None,
        });
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
    }
    assert_eq!(
        fixture
            .service
            .plan_store
            .taskless_plan_context_candidates_for_tab(&ambiguous)
            .len(),
        2,
        "two active plans claim this tab; the store reports both rather than picking"
    );

    let ledger = ledger_on(&mut fixture);
    for tab in [&unbound, &ambiguous] {
        fixture.service.record_kpi_event(tab, KpiEventKind::Poll, 8);
    }
    fixture.service.kpi.drain();

    for row in ledger.list_all().unwrap() {
        assert_eq!(
            row.plan_id, None,
            "tab {} must stay unassigned, not be charged to a chosen plan",
            row.tab_id
        );
        assert_eq!(row.task_id, None);
    }
}

/// The card's own reading: machine-wide, token_usage only, grouped and shared.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_groups_machine_token_usage_by_provider_and_model() {
    let mut fixture = fixture();
    let ledger = ledger_on(&mut fixture);
    ledger
        .insert_many(&[
            token_row("claude", Some("haiku"), 600, 200, 0),
            token_row("claude", Some("haiku"), 100, 40, 0),
            token_row("claude", Some("opus"), 100, 10, 100),
            token_row("codex", Some("gpt-5.5-codex"), 150, 50, 50),
            coordination_row("claude"),
        ])
        .unwrap();

    let spend = super::mission_control_ext::machine_spend(&fixture.service);
    assert_eq!(spend.scope, SpendScope::Machine);
    assert_eq!(spend.state, KpiLedgerState::Recording);
    assert_eq!(
        spend.measured_tokens, 1_100,
        "the send row contributes bytes, never tokens"
    );
    assert_eq!(spend.turns, 4, "turns count token_usage rows only");

    let claude = spend
        .providers
        .iter()
        .find(|row| row.provider == "claude")
        .unwrap();
    assert_eq!(claude.turns, 3);
    assert_eq!(claude.input_tokens, 800);
    assert_eq!(claude.uncached_input_tokens, 250);
    assert_eq!(claude.output_tokens, 100);

    let haiku = claude
        .models
        .iter()
        .find(|model| model.model.as_deref() == Some("haiku"))
        .unwrap();
    assert_eq!(haiku.turns, 2, "repeat turns on one model fold into one row");
    assert_eq!(haiku.input_tokens, 700);
    assert_eq!(haiku.uncached_input_tokens, 240);
    assert_eq!(haiku.output_tokens, 0);

    let shares: f32 = spend
        .providers
        .iter()
        .flat_map(|provider| provider.models.iter())
        .map(|model| model.token_share_pct)
        .sum();
    assert!(
        (shares - 100.0).abs() < 0.01,
        "model shares divide the measured token total, got {shares}"
    );
    let codex = model_row(&spend.providers, "codex");
    assert_eq!(codex.len(), 1);
    assert!(
        (codex[0].token_share_pct - 18.181818).abs() < 0.01,
        "share is measured tokens over the machine total, not an allowance slice"
    );
}

/// Provider percentages are consumed readings at provider/window granularity, and the
/// projection copies them without inverting or re-scaling.
#[test]
fn mission_control_spend_reports_provider_windows_as_allowance_used() {
    let row = codeterm_types::usage::UsageSnapshotRow {
        id: 1,
        provider: "claude".into(),
        account_id: None,
        captured_at: 1_700_000_000_000,
        parsed_ok: true,
        session_pct: Some(58.0),
        session_resets_at: Some(1_700_000_100_000),
        weekly_pct: Some(34.0),
        weekly_resets_at: Some(1_700_000_200_000),
        spend_cents: None,
        spend_limit_cents: None,
        spend_remaining_cents: None,
        spend_resets_at: None,
        failure_reason: Some("stale".into()),
        probe_command: None,
        raw_excerpt: None,
        debug_json: None,
    };

    let allowance = super::mission_control_ext::allowance_from_row(&row);
    assert_eq!(allowance.session_used_pct, row.session_pct);
    assert_eq!(allowance.weekly_used_pct, row.weekly_pct);
    assert_eq!(allowance.session_resets_at, row.session_resets_at);
    assert_eq!(allowance.weekly_resets_at, row.weekly_resets_at);
    assert_eq!(allowance.captured_at, row.captured_at);
    assert_eq!(allowance.stale_reason, row.failure_reason);

    let payload = serde_json::to_value(&allowance).unwrap();
    assert_eq!(payload["session_used_pct"], 58.0);
    assert_eq!(payload["weekly_used_pct"], 34.0);
    assert!(
        payload.get("session_remaining_pct").is_none(),
        "the stored figure is consumed; the wire shape must not rename it to remaining"
    );
}

/// Disabled, unreadable and empty ledgers each keep their own explicit state.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_states_stay_explicit_without_ledger_rows() {
    use codeterm_plan::service_plan::state::mission_control::KPI_ENABLED_SETTING;

    let mut fixture = fixture();
    fixture.service.kpi = KpiRecorder::disabled();
    let disabled = super::mission_control_ext::machine_spend(&fixture.service);
    assert_eq!(disabled.state, KpiLedgerState::NotRecording);
    assert_eq!(
        disabled.not_recording_reason,
        Some(KpiNotRecordingReason::Disabled)
    );
    assert_eq!(disabled.enable_setting, Some(KPI_ENABLED_SETTING));
    assert!(disabled.providers.is_empty());
    assert_eq!(disabled.measured_tokens, 0);

    let blocked = fixture.temp.path().join("blocked-spend-dir");
    std::fs::write(&blocked, b"not a directory").unwrap();
    fixture.service.kpi =
        KpiRecorder::from_config(&crate::config::schema::AppConfig::default(), &blocked);
    let unreadable = super::mission_control_ext::machine_spend(&fixture.service);
    assert_eq!(
        unreadable.not_recording_reason,
        Some(KpiNotRecordingReason::Unreadable)
    );
    assert_eq!(
        unreadable.enable_setting, None,
        "a failed open is not fixed by the opt-in setting"
    );

    ledger_on(&mut fixture);
    let empty = super::mission_control_ext::machine_spend(&fixture.service);
    assert_eq!(empty.state, KpiLedgerState::NotRecording);
    assert!(empty.providers.is_empty());
    assert_eq!(empty.turns, 0);
}

/// The route's own payload, so the Board reads a machine scope it cannot mistake for
/// a plan total.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_reaches_the_plan_state_route_as_a_machine_scope() {
    let mut fixture = fixture();
    let ledger = ledger_on(&mut fixture);
    ledger
        .insert_many(&[token_row("claude", Some("haiku"), 100, 10, 10)])
        .unwrap();

    let mut plan = Plan::new(
        "Machine scope".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some("tab-orch".into()),
            session_id: None,
            title: None,
        },
    );
    plan.status = PlanStatus::Active;
    fixture.service.plan_store.save(&mut plan, 1).unwrap();

    let snapshot = fixture.service.build_plan_state(&plan.id, NOW).unwrap();
    let payload = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(payload["mission_control"]["spend"]["scope"], "machine");
    assert_eq!(payload["mission_control"]["spend"]["state"], "recording");
    assert_eq!(payload["mission_control"]["spend"]["measured_tokens"], 110);
    assert_eq!(
        payload["mission_control"]["kpi"]["state"], "not_recording",
        "the machine figure never fills the plan-scoped block"
    );
}

/// The regression this exists for: ingestion used to hang off a tab status VALUE
/// changing, so a tab that finished turn after turn without ever flipping status
/// recorded nothing at all. The turn boundary is the live classifier settling to
/// idle, and that alone has to produce one pass. Every other fixture in this file
/// calls `request_kpi_usage_ingest` itself, so only this one proves a caller.
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn mission_control_spend_ingests_a_settled_turn_with_no_status_transition() {
    let mut fixture = fixture();
    let session = fixture.temp.path().join("session.jsonl");
    std::fs::write(
        &session,
        include_str!("../../agent/kpi/usage/fixtures/claude-session.jsonl"),
    )
    .unwrap();
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session_file("turn-mock", session)
            .with_session_model("sess-turn", "claude-opus-5"),
    ));

    let pty_id = "pty-turn";
    let workspace = fixture
        .service
        .mux
        .write()
        .create_workspace("turn".into(), None)
        .id;
    let tab_id = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        mux.link_pty(&tab_id, pty_id.to_string());
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("turn-mock".into());
        tab.cwd = Some(fixture.temp.path().display().to_string());
        tab.agent_session_id = Some("sess-turn".into());
        // Anything but idle, so settling to idle is a boundary the classifier crosses.
        tab.agent_state = Some(
            TabVerdict {
                state: TimelineState::Question,
                subtype: None,
            }
            .into(),
        );
    }

    // The composer's ready glyph: the screen the classifier reads as a settled turn.
    let hash = {
        let mut screen = fixture.service.screen_state.lock();
        screen.process(pty_id, "assistant finished\r\n\u{276f} \r\n".as_bytes());
        screen.get_content_hash(pty_id, 0).unwrap()
    };
    // One observation, so the tab has a quiescence reading. Status is Working here
    // and stays Working: nothing in this test moves it.
    {
        let mut tracker = fixture.service.agent_tracker.lock();
        tracker.update_output(pty_id, hash);
    }
    let status_before = fixture.service.agent_tracker.lock().get_status(pty_id);

    let ledger = ledger_on(&mut fixture);
    fixture.service.recompute_agent_state(pty_id);
    fixture.service.kpi.drain();

    let settled = fixture
        .service
        .mux
        .read()
        .get_tab(&tab_id)
        .and_then(|tab| tab.agent_state.clone())
        .map(|state| state.state);
    assert_eq!(
        settled,
        Some(TimelineState::Idle),
        "the fixture screen must actually settle the turn, or this proves nothing"
    );
    assert_eq!(
        fixture.service.agent_tracker.lock().get_status(pty_id),
        status_before,
        "the tab status never moved; the turn boundary alone drove the pass"
    );

    let usage: Vec<_> = ledger
        .list_all()
        .unwrap()
        .into_iter()
        .filter(|row| row.kind == KpiEventKind::TokenUsage)
        .collect();
    assert!(
        !usage.is_empty(),
        "a settled turn records the provider's usage rows"
    );
    assert!(
        usage.iter().all(|row| row.tab_id == tab_id),
        "the rows are charged to the tab whose turn settled"
    );
}
