use super::*;
use crate::plan::{CuratorRound, HeartbeatLearning, PlanMode, TaskExecutionAttempt, TaskStatus};
use crate::service_plan::curator::{digest, movement, synthesis};
use std::collections::BTreeMap;

fn milestone(id: &str, kind: &str, text: &str, ts: u64) -> crate::plan::Milestone {
    serde_json::from_value(
        json!({"id":id,"from":{"tab_id":"owner"},"kind":kind,"text":text,"ts":ts}),
    )
    .unwrap()
}

fn rows(plan: &Plan, since: u64, now: u64, full: bool) -> BTreeMap<String, Vec<digest::Row>> {
    let mut rows = if full {
        digest::plan_rows(plan, since)
    } else {
        digest::metadata_rows(plan, since)
    };
    rows.extend(if full {
        movement::rows(plan, since, now)
    } else {
        movement::current_rows(plan, since, now)
    });
    rows
}

#[test]
fn curator_sql_rows_match_distinct_order_counts_synthesis_and_digest_budgets() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Digest".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.control_loop.last_round = Some(CuratorRound {
        ended_at: 50,
        ..Default::default()
    });
    for index in 0..24 {
        let mut task = crate::service_plan::load_audit::task(
            &format!("task_{index:02}"),
            if index < 2 {
                TaskStatus::Done
            } else {
                TaskStatus::InReview
            },
            None,
        );
        task.status_changed_at = 100 + index;
        task.review_evidence=serde_json::from_value(json!([
            {"id":format!("review-{index}-a"),"from":{"tab_id":"owner"},"verdict":"hold","at":100,"note":format!("{}a","é|\n".repeat(120))},
            {"id":format!("review-{index}-b"),"from":{"tab_id":"owner"},"verdict":"bounce","at":101,"note":"latest"}
        ])).unwrap();
        let report=serde_json::from_value(json!({"id":format!("report-{index}"),"status":"in_review","sha":"abc","message":"opaque\nTESTS COVERING CHANGES: opaque\r\n",
            "loaded_plan_skills":[{"slug":"shared","content_digest":"opaque","loaded_at":10}]})).unwrap();
        task.execution_attempts = vec![TaskExecutionAttempt {
            id: format!("attempt-{index}"),
            assigned_at: Some(100),
            reported_at: Some(110),
            report: Some(report),
            closed_at: Some(120),
            ..Default::default()
        }];
        plan.tasks.push(task);
    }
    plan.tasks.push(crate::service_plan::load_audit::task(
        "progress",
        TaskStatus::InProgress,
        None,
    ));
    let mut missing =
        crate::service_plan::load_audit::task("missing-dependency", TaskStatus::Pending, None);
    missing.depends_on = vec!["absent".into()];
    plan.tasks.push(missing);
    for index in 0..30 {
        plan.strides.push(milestone(
            &format!("report-{index:02}"),
            "worker_report",
            &format!("{}tail-{index}", "same-prefix".repeat(20)),
            100 + index,
        ));
        plan.tasks[0].milestones.push(milestone(
            &format!("memory-{index:02}"),
            "skill_ledger",
            "memorized",
            100 + index,
        ));
    }
    plan.strides
        .push(milestone("decision", "decision_answer", "opaque", 100));
    plan.strides.push(milestone(
        "decision-max",
        "decision_answer",
        "opaque",
        u64::MAX,
    ));
    plan.heartbeat.learnings = vec![HeartbeatLearning {
        id: "carried".into(),
        text: "verify task_02".into(),
        ts: 1,
        tag: Some("curator:next".into()),
    }];
    plan.elements = serde_json::from_value(json!([
        {"id":"handoff-one","kind":"handoff","content":{"ts":10,"body":"unrelated prose"}},
        {"id":"handoff-max","kind":"handoff","content":{"ts":u64::MAX}},
        {"id":"handoff-negative","kind":"handoff","content":{"ts":-1}},
        {"id":"handoff-float","kind":"handoff","content":{"ts":123.5}},
        {"id":"handoff-string","kind":"handoff","content":{"ts":"999"}},
        {"id":"other","kind":"note","content":{"ts":999}}
    ]))
    .unwrap();
    let database = fixtures::database(directory.path(), &plan);
    let read = database.curator_digest_projection(&plan.id, 1000).unwrap();
    assert_eq!(read.latest_handoff_at(), Some(u64::MAX));
    assert_eq!(
        read.latest_handoff_at(),
        plan.elements
            .iter()
            .filter(|element| element.kind == "handoff")
            .filter_map(|element| element
                .content
                .get("ts")
                .and_then(serde_json::Value::as_u64))
            .max()
    );
    let mut full = rows(&plan, 50, 1000, true);
    let mut extra: BTreeMap<String, Vec<digest::Row>> = BTreeMap::new();
    let duplicate = digest::Row {
        id: "report-00".into(),
        kind: "worker_report".into(),
        state: "recorded".into(),
        summary: format!("{}tail-0", "same-prefix".repeat(20)),
    };
    full.entry("reports".into())
        .or_default()
        .push(duplicate.clone());
    extra.entry("reports".into()).or_default().push(duplicate);
    for limit in [1, 32, 256, 1024, 8192] {
        let actual = read.build_digest("header", &extra, limit).unwrap();
        let expected = digest::build_bounded("header", &full, limit);
        assert_eq!(actual.body, expected.body, "limit={limit}");
        assert_eq!(actual.activity, expected.activity);
        assert_eq!(actual.section_sizes, expected.section_sizes);
    }
    assert_eq!(read.group_close(), synthesis::group_close(&plan));
}

#[test]
fn curator_materialization_is_bounded_and_synthesis_keeps_old_fallback_learning() {
    let mut prior = None;
    for (strides, attempts, extra_body) in [(0, 1, 0), (2048, 1, 0), (0, 2048, 0), (0, 1, 32768)] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.mode = PlanMode::Execution;
        plan.control_loop.last_round = Some(CuratorRound {
            ended_at: 100,
            ..Default::default()
        });
        plan.tasks[1].body.push_str(&"x".repeat(extra_body));
        for number in 0..100 {
            let round = CuratorRound {
                number,
                ended_at: number * 1000,
                digest_hash: "opaque".into(),
                summary: format!("move-{number}"),
                ..Default::default()
            };
            plan.strides.push(milestone(
                &format!("round-{number:03}"),
                "curator_round",
                &serde_json::to_string(&round).unwrap(),
                number * 1000,
            ));
        }
        plan.strides
            .push(milestone("old-close", "group_close", "opaque", 20));
        plan.strides.extend((0..256).map(|index| {
            milestone(
                &format!("anchor-{index:03}"),
                "note",
                "fixed",
                200_000 + index,
            )
        }));
        plan.heartbeat.learnings = vec![HeartbeatLearning {
            id: "fallback".into(),
            text: "+ old round fallback".into(),
            ts: 99_000,
            tag: Some("curator".into()),
        }];
        plan.heartbeat
            .learnings
            .extend((0..60).map(|index| HeartbeatLearning {
                id: format!("recent-{index}"),
                text: "recent".into(),
                ts: 200_000 + index,
                tag: None,
            }));
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        super::super::super::super::store_sync::take_decoded_row_bytes();
        let full = database.load_plan(&plan.id).unwrap().unwrap();
        let baseline_decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        take_metrics();
        super::super::super::super::store_rows::take_serialized_row_bytes();
        let read = database
            .curator_digest_projection(&plan.id, 300_000)
            .unwrap();
        let header = read.synthesis_context_header("synthesis: yes").unwrap();
        let extra = BTreeMap::new();
        let actual = read.build_digest(&header, &extra, 8192).unwrap();
        let metrics = take_metrics();
        let decoded = super::super::super::super::store_sync::take_decoded_row_bytes();
        let serialized = super::super::super::super::store_rows::take_serialized_row_bytes();
        assert_eq!(header, synthesis::context_header(&full, "synthesis: yes"));
        let expected = digest::build(&header, &rows(&full, 100, 300_000, true));
        assert_eq!(actual.body, expected.body);
        assert_eq!(actual.activity, expected.activity);
        assert_eq!(
            read.real_rounds_started(),
            synthesis::real_rounds_started(&full)
        );
        assert_eq!(read.real_rounds_started(), 100);
        assert_eq!(read.group_close(), Some(20));
        assert!(header.contains("old round fallback"));
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        assert!(metrics.materialized_bytes < actual.body.len() + 16 * 1024);
        assert!(decoded < actual.body.len() + 4096);
        println!("fixture={} consumer=curator_digest tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} extra_body={extra_body} anchor_strides=256 valid_rounds=100 retained_rounds=64 baseline_decoded_json_bytes={baseline_decoded} queries={} materialized_text_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes,actual.body.len());
        let measured = (metrics, decoded, actual.body.len());
        if let Some(prior) = &prior {
            assert_eq!(&measured, prior);
        }
        prior = Some(measured);
    }
}

#[test]
fn curator_snapshot_allows_writer_callbacks_without_torn_digest_or_silent_decode_errors() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "A".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.tasks = vec![crate::service_plan::load_audit::task(
        "one",
        TaskStatus::Pending,
        None,
    )];
    plan.strides = vec![milestone("report", "worker_report", "old text", 20)];
    let database = fixtures::database(directory.path(), &plan);
    let read = database.curator_digest_projection(&plan.id, 1000).unwrap();
    {
        let mut writer = database.connection.lock();
        let tx = writer.transaction().unwrap();
        tx.execute(
            "UPDATE plans SET title='B',header_json=json_set(header_json,'$.title','B') WHERE id=?1",
            [&plan.id],
        )
        .unwrap();
        tx.execute("UPDATE plan_strides SET row_json=json_set(row_json,'$.text','new text') WHERE plan_id=?1",[&plan.id]).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(read.scope().title, "A");
    assert!(read
        .build_digest("header", &BTreeMap::new(), 8192)
        .unwrap()
        .body
        .contains("old text"));
    let latest = database.curator_digest_projection(&plan.id, 1000).unwrap();
    assert_eq!(latest.scope().title, "B");
    assert!(latest
        .build_digest("header", &BTreeMap::new(), 8192)
        .unwrap()
        .body
        .contains("new text"));
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=json_remove(json_set(row_json,'$.status_changed_at',1001),'$.name') WHERE plan_id=?1",
            [&plan.id],
        )
        .unwrap();
    let malformed = database.curator_digest_projection(&plan.id, 1000).unwrap();
    assert!(malformed
        .build_digest("header", &BTreeMap::new(), 8192)
        .is_err());
    assert!(matches!(
        database.curator_digest_projection("absent", 1000),
        Err(PlanError::NotFound)
    ));
}

#[test]
fn curator_runtime_notices_keep_action_needed_answers_and_done_confirmation_rules() {
    use crate::service_plan::CuratorRuntimeNotice;
    let directory = tempfile::tempdir().unwrap();
    let mut plan = Plan::new(
        "Runtime".into(),
        directory.path().display().to_string(),
        OrchestratorRef::default(),
    );
    plan.control_loop.last_round = Some(CuratorRound {
        ended_at: 50,
        ..Default::default()
    });
    plan.control_loop.observed_owner_answers = ["already-seen".into()].into();
    plan.tasks = vec![
        crate::service_plan::load_audit::task("done-one", TaskStatus::Done, None),
        crate::service_plan::load_audit::task("done-two", TaskStatus::Done, None),
    ];
    for task in &mut plan.tasks {
        task.status_changed_at = 100;
    }
    let database = fixtures::database(directory.path(), &plan);
    let read = database.curator_digest_projection(&plan.id, 1000).unwrap();
    let notices = vec![
        CuratorRuntimeNotice {
            id: "open-question".into(),
            timestamp: 200,
            action_needed: true,
            answered: false,
            resolved: false,
            title: "question".into(),
            body: None,
        },
        CuratorRuntimeNotice {
            id: "new-answer".into(),
            timestamp: 1,
            action_needed: true,
            answered: true,
            resolved: false,
            title: "answered".into(),
            body: None,
        },
        CuratorRuntimeNotice {
            id: "not-action-needed".into(),
            timestamp: 200,
            action_needed: false,
            answered: true,
            resolved: false,
            title: "done-one accepted".into(),
            body: None,
        },
        CuratorRuntimeNotice {
            id: "already-seen".into(),
            timestamp: 200,
            action_needed: true,
            answered: true,
            resolved: false,
            title: "old answer".into(),
            body: None,
        },
    ];
    let mut expected = rows(&plan, 50, 1000, true);
    for (section, id, kind, state, summary) in [
        (
            "owner questions",
            "open-question",
            "question",
            "open",
            "800",
        ),
        (
            "decisions",
            "new-answer",
            "owner_answer",
            "newly_observed",
            "1000",
        ),
        (
            "done but not shown",
            "done-two",
            "accepted",
            "notice_unconfirmed",
            "100",
        ),
    ] {
        expected
            .entry(section.into())
            .or_default()
            .push(digest::Row {
                id: id.into(),
                kind: kind.into(),
                state: state.into(),
                summary: summary.into(),
            });
    }
    let actual = read
        .build_digest_with_runtime("header", &BTreeMap::new(), 8192, Some(&notices))
        .unwrap();
    let expected = digest::build("header", &expected);
    assert_eq!(actual.body, expected.body);
    assert_eq!(actual.activity, expected.activity);
    assert_eq!(actual.section_sizes, expected.section_sizes);
}
