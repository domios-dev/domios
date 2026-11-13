use super::*;
use crate::agent::kpi::KpiRecorder;
use crate::agent::kpi_ledger::{KpiEventKind, KpiLedger};
use crate::multiplexer::types::AgentRole;
use crate::service::agent_runtime::engine::test_support::runtime_fixture_with;

fn turn(thread: &str, state: RuntimeTurnState) -> AgentRuntimeEvent {
    AgentRuntimeEvent::TurnState {
        thread_id: thread.into(),
        turn_id: Some("usage-turn".into()),
        parent_thread_id: None,
        state,
        detail: None,
        status_line: None,
        started_at: None,
        completed_at: None,
    }
}

#[test]
fn runtime_completion_ingests_provider_transcript_once_without_a_pty() {
    let directory = tempfile::tempdir().unwrap();
    let transcript = directory.path().join("rollout.jsonl");
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../../../../../../plugins/provider-shared/tests/fixtures/token-analytics.json"
    ))
    .unwrap();
    let case = cases.iter().find(|case| case["runtime"] == true).unwrap();
    let source_path = crate::test_support::fixture_manifest_dir()
        .join("../../..")
        .join(case["source"].as_str().unwrap());
    std::fs::copy(source_path, &transcript).unwrap();
    let source = format!(
        "module.exports = {{ usageSources() {{ return [{{path:{}}}]; }} }};",
        serde_json::to_string(&transcript).unwrap()
    );
    let (mut fixture, tab, _peer, _cleanup) = runtime_fixture_with(&source);
    let ledger = Arc::new(KpiLedger::open(directory.path()).unwrap());
    fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());
    {
        let mut mux = fixture.service.mux.write();
        let target = mux.get_tab_mut(&tab).unwrap();
        target.role = Some(AgentRole::Worker);
        target.pty_id = None;
    }
    let session = engine::for_tab(&tab).unwrap();
    let context = session.context.lock().clone();
    let thread = context.thread_id.as_deref().unwrap();
    let publish = |event| {
        crate::async_runtime::block_on(publish_events(
            &fixture.service,
            &session,
            context.epoch,
            vec![event],
            1_789_000_000_000,
        ))
        .unwrap();
        fixture.service.kpi.drain();
    };

    publish(turn(thread, RuntimeTurnState::Working));
    assert!(ledger.list_all().unwrap().is_empty());
    publish(turn(thread, RuntimeTurnState::Idle));
    let recorded = ledger.list_all().unwrap();
    assert_eq!(recorded.len(), 3);
    assert!(recorded.iter().all(|row| {
        row.kind == KpiEventKind::TokenUsage
            && row.tab_id == tab
            && row.provider == session.provider
            && row.role.as_deref() == Some("Worker")
    }));
    assert_eq!(
        recorded
            .iter()
            .map(|row| row.tokens.unwrap().input)
            .sum::<u64>(),
        50_000
    );
    assert_eq!(
        recorded
            .iter()
            .map(|row| row.tokens.unwrap().output)
            .sum::<u64>(),
        700
    );

    publish(turn(thread, RuntimeTurnState::Idle));
    assert_eq!(ledger.list_all().unwrap(), recorded);
    fixture.service.kpi.rebuild_usage().unwrap();
    assert_eq!(ledger.list_all().unwrap(), recorded);
}
