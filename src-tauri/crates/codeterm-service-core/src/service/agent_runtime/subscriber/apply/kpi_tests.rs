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
    let parsed: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(&transcript)
        .unwrap()
        .lines()
        .filter_map(|line| {
            let record: serde_json::Value = serde_json::from_str(line).ok()?;
            let parsed = codeterm_plugin::token_usage::cumulative::parse_record(&record, 0)?;
            Some((
                record["timestamp"].as_str()?.to_owned(),
                serde_json::to_value(parsed).unwrap(),
            ))
        })
        .collect();
    let source = format!(
        "module.exports = {{ usageAccounting() {{ return 'transcript'; }}, parseUsageRecord(record) {{ return ({})[record.timestamp] ?? null; }}, usageSources() {{ return [{{path:{}}}]; }} }};",
        serde_json::to_string(&parsed).unwrap(), serde_json::to_string(&transcript).unwrap()
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

#[test]
fn runtime_usage_contract_archives_only_declared_runtime_responses_once() {
    for (mode, expected) in [("runtime_turn", 1), ("transcript", 0), ("unreported", 0)] {
        let source = format!("module.exports = {{ usageAccounting() {{ return '{mode}'; }} }};");
        let (mut fixture, tab, _peer, _cleanup) = runtime_fixture_with(&source);
        let directory = tempfile::tempdir().unwrap();
        let ledger = Arc::new(KpiLedger::open(directory.path()).unwrap());
        fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());
        let session = engine::for_tab(&tab).unwrap();
        let context = session.context.lock().clone();
        let event = |response| AgentRuntimeEvent::Usage {
            thread_id: context.thread_id.clone().unwrap(),
            usage: TokenUsage {
                response_id: response,
                input_tokens: 20,
                output_tokens: 6,
                cache_read_tokens: Some(5),
                cache_write_tokens: None,
                used_tokens: None,
                window_tokens: None,
                cost_micros: None,
            },
        };
        for response in [None, Some("own-prompt".into()), Some("own-prompt".into())] {
            crate::async_runtime::block_on(publish_events(
                &fixture.service,
                &session,
                context.epoch,
                vec![event(response)],
                123_000,
            ))
            .unwrap();
        }
        fixture.service.kpi.drain();
        let rows = ledger.list_all().unwrap();
        assert_eq!(rows.len(), expected, "{mode}");
        if expected > 0 {
            assert_eq!(rows[0].tokens.unwrap().uncached_input, 15);
            assert_eq!(rows[0].timestamp, 123);
        }
        fixture.service.kpi.rebuild_usage().unwrap();
        assert_eq!(ledger.list_all().unwrap(), rows);
    }
}
