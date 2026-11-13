use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::types::AgentRole;
use crate::plan::PlanMode;
use crate::service::plan::NewTask;

#[test]
fn analytics_mixed_provider_transcripts_keep_team_task_and_role_after_rebuild() {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = ServiceFixture::new(mux);
    let ledger = Arc::new(KpiLedger::open(fixture.temp.path()).unwrap());
    fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());
    let svc = &fixture.service;
    let workspace = {
        let mut mux = svc.mux.write();
        mux.set_limit_gate(codeterm_entitlements::LimitGate::dormant());
        let workspace = mux.create_workspace("Usage".into(), None).id;
        mux.upsert_team(Team {
            id: "mixed-team".into(),
            name: Some("Mixed".into()),
            workspace_id: workspace.clone(),
            locked: false,
            lifecycle: Default::default(),
            created_at: 0,
        });
        workspace
    };
    let mut plan = Plan::new(
        "Mixed usage".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab("usage-owner"),
    );
    plan.team_id = Some("mixed-team".into());
    plan.mode = PlanMode::Approved;
    svc.plan_store.save(&mut plan, 1).unwrap();

    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../../../../plugins/provider-shared/tests/fixtures/token-analytics.json"
    ))
    .unwrap();
    let mut expected = Vec::new();
    for case in &cases {
        let provider = case["provider"].as_str().unwrap();
        let session = case["session"].as_str().unwrap();
        let source_path = crate::test_support::fixture_manifest_dir()
            .join("../../..")
            .join(case["source"].as_str().unwrap());
        let path = fixture.temp.path().join(provider);
        let provider = if case["format"] == "sqlite" {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(&std::fs::read_to_string(source_path).unwrap())
                .unwrap();
            MockProvider::new(provider).with_usage_source(path, session)
        } else {
            std::fs::copy(source_path, &path).unwrap();
            MockProvider::with_session_file(provider, path)
        };
        svc.agent_registry.register_provider(Box::new(provider));
        let provider = case["provider"].as_str().unwrap();
        let tab = svc.mux.write().create_tab(&workspace).unwrap();
        {
            let mut mux = svc.mux.write();
            let target = mux.get_tab_mut(&tab).unwrap();
            target.agent_type = Some(provider.into());
            target.agent_session_id = Some(session.into());
            target.cwd = Some(fixture.temp.path().display().to_string());
            target.role = Some(AgentRole::Worker);
        }
        let task = svc
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: provider.into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        svc.plan_store
            .set_task_assignment(&task, Some(&tab), Some(provider), 3)
            .unwrap();
        svc.request_kpi_usage_ingest(&tab);
        expected.push((case, tab, task));
    }
    svc.kpi.drain();
    let recorded = ledger.list_all().unwrap();
    for (case, tab, task) in &expected {
        let provider = case["provider"].as_str().unwrap();
        let rows: Vec<_> = recorded
            .iter()
            .filter(|row| row.kind == KpiEventKind::TokenUsage && row.provider == provider)
            .collect();
        assert_eq!(rows.len() as u64, case["totals"]["turns"].as_u64().unwrap());
        assert!(rows.iter().all(|row| {
            row.tab_id == *tab
                && row.plan_id.as_deref() == Some(plan.id.as_str())
                && row.task_id.as_deref() == Some(task.as_str())
                && row.role.as_deref() == Some("Worker")
                && row.workspace_id.as_deref() == Some(workspace.as_str())
        }));
    }
    let query = json!({
        "range":{"from":"2026-05-20","to":"2026-07-21","tz":"UTC"},
        "bucket":"week","group_by":"provider_model","filters":{"teams":["mixed-team"]}
    })
    .to_string();
    let snapshot = without_clock(native_reply(svc, &query));
    assert_eq!(
        snapshot["tokens"]["totals"],
        json!({"new_input":37_600,"generated":3_320,"read_from_cache":581_724,"turns":8})
    );
    let groups = snapshot["tokens"]["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 3);
    for (case, _, _) in &expected {
        let group = groups
            .iter()
            .find(|row| row["key"]["id"] == case["provider"])
            .unwrap();
        assert_eq!(group["totals"], case["totals"]);
    }
    assert_eq!(without_clock(js_reply(svc, &query)), snapshot);
    for _ in 0..2 {
        svc.kpi.rebuild_usage().unwrap();
        assert_eq!(ledger.list_all().unwrap(), recorded);
        assert_eq!(without_clock(native_reply(svc, &query)), snapshot);
    }
}
