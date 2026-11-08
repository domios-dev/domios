use super::*;
use crate::plan::reconcile::TabProbe;
use crate::plan::{PlanStatus, PlannerRef};

fn probe(session: Option<&str>, live: bool) -> TabProbe {
    TabProbe {
        exists: true,
        live_agent: live,
        session_binding_valid: true,
        chat_delivery_target: true,
        agent_accepting_work: true,
        agent_session_id: session.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn coordinator_and_restoration_facts_preserve_archives_and_session_fences() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Role".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: "planner".into(),
        session_id: Some("session".into()),
        title: Some("Planner".into()),
    });
    let database = fixtures::database(directory.path(), &plan);
    for archived in [false, true] {
        plan.status = if archived {
            PlanStatus::Archived
        } else {
            PlanStatus::Active
        };
        database.persist_plan(&plan, None).unwrap();
        for (session, live) in [
            (Some("session"), true),
            (Some("foreign"), true),
            (None, true),
            (Some("session"), false),
        ] {
            let probe = probe(session, live);
            for tab in ["owner", "planner", "other"] {
                let expected = plan.owner.tab_id.as_deref() == Some(tab)
                    || matches!(crate::plan::peer::resolve_planner_ref(&plan.planner,&plan.owner,|_|probe.clone()),crate::plan::peer::PeerResolution::Planner {tab_id} if tab_id==tab);
                assert_eq!(
                    database
                        .coordinator_binding_projection(
                            tab,
                            probe.agent_session_id.as_deref(),
                            probe.planner_routable_for(probe.agent_session_id.as_deref())
                        )
                        .unwrap(),
                    expected
                );
            }
        }
        let owner = database.restored_plan_bindings_projection("owner").unwrap();
        assert_eq!(owner.len(), 1);
        assert!(owner[0].owns_plan);
        assert!(owner[0].planner_binding.is_none());
        let planner = database
            .restored_plan_bindings_projection("planner")
            .unwrap();
        assert_eq!(planner.len(), 1);
        assert!(!planner[0].owns_plan);
        assert_eq!(planner[0].planner_binding, plan.planner);
        assert!(database
            .restored_plan_bindings_projection("absent")
            .unwrap()
            .is_empty());
    }
    database.connection.lock().execute("UPDATE plans SET header_json=json_set(header_json,'$.planner.session_id',17) WHERE id=?1",[&plan.id]).unwrap();
    assert!(database
        .coordinator_binding_projection("planner", Some("session"), true)
        .is_err());
    assert!(matches!(
        database.restored_plan_bindings_projection("planner"),
        Err(PlanError::Quarantined { .. })
    ));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,'$.owner.tab_id',17) WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
    assert!(database
        .coordinator_binding_projection("17", None, false)
        .is_err());
    assert!(database.restored_plan_bindings_projection("17").is_err());
}

#[test]
fn role_binding_materialization_excludes_task_history_and_unused_owner_fields() {
    let mut previous = None;
    for (strides, attempts, body, rounds) in [
        (0, 1, 0, 0),
        (2048, 1, 0, 0),
        (0, 2048, 0, 0),
        (0, 1, 32768, 0),
        (0, 1, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.owner.tab_id = Some("owner".into());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.strides.extend(fixtures::round_strides(
            (0..rounds)
                .map(|number| crate::plan::CuratorRound {
                    number,
                    summary: "unrelated history".repeat(16),
                    ..Default::default()
                })
                .collect(),
        ));
        let database = fixtures::database(directory.path(), &plan);
        database
            .connection
            .lock()
            .execute(
                "UPDATE plans SET header_json=json_set(header_json,'$.owner.title',?2) WHERE id=?1",
                params![plan.id, "unused".repeat(body)],
            )
            .unwrap();
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let owner = database
            .coordinator_binding_projection("owner", None, false)
            .unwrap();
        assert!(owner);
        let bindings = database.restored_plan_bindings_projection("owner").unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(metrics.queries, 2);
        assert!(metrics.materialized_bytes < 512);
        assert!(decoded < 512);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(owner, bindings)).unwrap().len();
        println!("fixture={} consumer=coordinator_restore_binding tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}
