use super::*;
use crate::plan::{CuratorRound, CuratorRun, Milestone, TaskStatus};

fn stride(id: &str, text: String) -> Milestone {
    Milestone {
        id: id.into(),
        from: Default::default(),
        kind: "curator_round".into(),
        text,
        ts: 1,
        links: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
        attachments: vec![],
    }
}

#[test]
fn curator_lifecycle_reads_select_constant_bytes_without_writes() {
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
        plan.tasks[0].status = TaskStatus::Done;
        plan.tasks[0].execution_attempts[0].report = Some(serde_json::from_value(json!({"id":"report","loaded_plan_skills":[{"slug":"selected","content_digest":"opaque","loaded_at":1}]})).unwrap());
        plan.tasks[0].body.push_str(&"x".repeat(body));
        plan.control_loop.real_rounds_started = 7;
        plan.control_loop.running = Some(CuratorRun {
            tab_id: Some("curator".into()),
            run_id: Some("run".into()),
            refused_actions: vec!["x".repeat(body)],
            ..Default::default()
        });
        plan.control_loop.last_round = Some(CuratorRound {
            tab_id: Some("other".into()),
            summary: "x".repeat(body),
            ..Default::default()
        });
        plan.strides.extend(fixtures::round_strides(vec![
            CuratorRound::default();
            rounds
        ]));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        super::super::super::super::store_sync::take_decoded_row_bytes();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let episodes = database
            .curator_skill_episodes_projection(&plan.id, "selected")
            .unwrap();
        let count = database.curator_real_rounds_projection(&plan.id).unwrap();
        let closed = database
            .curator_closed_tab_projection(&plan.id, "curator")
            .unwrap();
        let identity = database.plan_identity_projection(&plan.id).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(episodes, vec![plan.tasks[0].id.clone()]);
        assert_eq!(count, 7);
        assert_eq!(closed, (true, Some("run".into()), None));
        assert_eq!(identity.owner, plan.owner);
        assert_eq!(metrics.queries, 4);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let response = serde_json::to_vec(&(episodes, count, closed, identity))
            .unwrap()
            .len();
        println!("fixture={} consumers=save_memory_for_caller,curator_spawned,settle_closed_curator_tab tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={body} unrelated_rounds={rounds} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0", fixtures::SEED, fixtures::TASKS, fixtures::CONTENT_BYTES, metrics.queries, metrics.materialized_bytes);
        let measured = (metrics, decoded, response);
        if let Some(previous) = &previous {
            assert_eq!(&measured, previous);
        }
        previous = Some(measured);
    }
}

#[test]
fn curator_lifecycle_round_count_preserves_legacy_validation_and_declared_maximum() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Curator".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let valid = CuratorRound {
        digest_hash: "opaque".into(),
        ..Default::default()
    };
    plan.strides = vec![
        stride("valid", serde_json::to_string(&valid).unwrap()),
        stride(
            "empty",
            serde_json::to_string(&CuratorRound::default()).unwrap(),
        ),
        stride("malformed", "{".into()),
        stride("wrong-schema", "{\"digest_hash\":\"opaque\"}".into()),
    ];
    let mut oversized = valid.clone();
    oversized.summary = "x".repeat(16384);
    plan.strides.push(stride(
        "oversized",
        serde_json::to_string(&oversized).unwrap(),
    ));
    for declared in [0, 1, 9, u64::MAX] {
        plan.control_loop.real_rounds_started = declared;
        let database = fixtures::database(directory.path(), &plan);
        assert_eq!(
            database.curator_real_rounds_projection(&plan.id).unwrap(),
            crate::service_plan::curator::synthesis::real_rounds_started(&plan)
        );
    }
}

#[test]
fn curator_closed_tab_preserves_running_precedence_last_round_and_storage_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Curator".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    let last = CuratorRound {
        tab_id: Some("curator".into()),
        run_id: Some("last".into()),
        ..Default::default()
    };
    plan.control_loop.last_round = Some(last.clone());
    plan.control_loop.running = Some(CuratorRun {
        tab_id: Some("curator".into()),
        ..Default::default()
    });
    let database = fixtures::database(directory.path(), &plan);
    assert_eq!(
        database
            .curator_closed_tab_projection(&plan.id, "curator")
            .unwrap(),
        (true, None, None)
    );
    plan.control_loop.running.as_mut().unwrap().tab_id = Some("other".into());
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database
            .curator_closed_tab_projection(&plan.id, "curator")
            .unwrap(),
        (false, None, Some(last))
    );
    assert_eq!(
        database
            .curator_closed_tab_projection(&plan.id, "unknown")
            .unwrap(),
        (false, None, None)
    );
    assert!(matches!(
        database.curator_closed_tab_projection("missing", "curator"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.curator_real_rounds_projection("missing"),
        Err(PlanError::NotFound)
    ));
    assert!(matches!(
        database.curator_skill_episodes_projection("missing", "skill"),
        Err(PlanError::NotFound)
    ));
    database
        .connection
        .lock()
        .execute_batch("DROP TABLE plan_tasks; DROP TABLE plan_strides; DROP TABLE plans")
        .unwrap();
    assert!(database
        .curator_closed_tab_projection(&plan.id, "curator")
        .is_err());
    assert!(database.curator_real_rounds_projection(&plan.id).is_err());
    assert!(database
        .curator_skill_episodes_projection(&plan.id, "skill")
        .is_err());
}
