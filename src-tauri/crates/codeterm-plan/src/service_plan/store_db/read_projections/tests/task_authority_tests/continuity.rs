use super::*;
use crate::plan::{Milestone, Task, TaskExecutionAttempt};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

fn rich_plan(root: &std::path::Path) -> Plan {
    let mut plan = authority_plan(root);
    let attachment = json!({"id":"attachment","name":"evidence","path":"artifact.txt","media_type":"text/plain","size_bytes":1,"created_at":1});
    let mut task = serde_json::to_value(&plan.tasks[0]).unwrap();
    for key in [
        "model",
        "speed",
        "model_rationale",
        "personality",
        "test_plan",
        "qa_plan",
        "correctness",
        "review_oracle",
        "status_label",
        "handle",
        "key",
        "role_guidance",
        "executor_role",
        "intent",
        "parent_task_id",
        "assigned_tab_id",
        "taken_by_role",
        "peer",
        "group",
        "pause_reason",
    ] {
        task[key] = json!("opaque");
    }
    task["body"] = json!("selected body");
    task["personality_provenance"] = json!("suggested");
    task["qa_state"] = json!("running");
    task["acceptance_criteria"] = json!(["accepted"]);
    task["external_refs"] = json!([{"kind":"github_issue","repository":"owner/repository","number":1,"canonical_url":"https://example.invalid/1","relationship":"relates_to"}]);
    task["review_evidence"] = json!([{"id":"evidence","from":{"tab_id":"reviewer","session_id":"session","title":"Reviewer"},"verdict":"accept","note":"reviewed","at":1}]);
    task["review_override"] =
        json!({"owner":"owner","reason":"reason","reproduction":"proof","at":1});
    task["review_audit"] = json!([{"id":"audit","owner":"owner","reason":"reason","reproduction":"proof","round":1,"at":1}]);
    task["tags"] = json!(["work:change"]);
    task["section_ids"] = json!(["section"]);
    task["depends_on"] = json!(["dependency"]);
    task["files"] = json!({"edit":["src/owned.rs"],"read":["contract.md"]});
    task["competition"] =
        json!({"enabled":true,"attempts":1,"models":["model"],"judge_hint":"judge"});
    task["canvas"] = json!({"x":1.0,"y":2.0,"collapsed":false});
    task["attachments"] = json!([attachment.clone()]);
    task["interruption"]["allowance"] =
        json!({"remaining_pct":90.0,"observed_at":1,"stale_after_ms":2,"reserve_pct":5.0});
    task["interruption"]["checkpoint"] =
        serde_json::to_value(crate::plan::InterruptionCheckpoint::default()).unwrap();
    let mut attempt = serde_json::to_value(&plan.tasks[0].execution_attempts[0]).unwrap();
    for key in [
        "executor_title",
        "provider",
        "agent_type",
        "model",
        "requested_model",
        "provider_resolved_model",
        "reasoning_effort",
        "actual_reasoning_effort",
        "session_model",
        "session_id",
        "session_binding_nonce",
        "role",
        "parent_tab_id",
        "orchestrator_tab_id",
        "mesh_peer",
        "remote_worktree_path",
        "remote_lane_tip",
        "remote_merge_commit",
        "branch",
        "worktree_path",
        "checkout_path",
        "shared_start_sha",
        "base",
        "destination",
        "closure_reason",
        "competition_group_id",
        "judge_verdict",
        "judge_reason",
    ] {
        attempt[key] = json!("opaque");
    }
    for key in [
        "assigned_at",
        "acknowledged_at",
        "reported_at",
        "report_target_written_at",
        "reviewed_at",
        "closed_at",
        "retired_at",
        "judged_at",
        "competition_lane",
    ] {
        attempt[key] = json!(1);
    }
    attempt["model_identity_mismatch"] = json!({"requested_model":"requested","provider_resolved_model":"resolved","actual_model":"actual","reason":"reason"});
    attempt["remote_dispatch_state"] = json!("acknowledged");
    attempt["authorized_gates"] = json!(["gate"]);
    attempt["placement"] = json!("shared");
    attempt["execution_intent"] = json!("mutating");
    attempt["report"] =
        serde_json::to_value(crate::plan::TaskExecutionReportSnapshot::default()).unwrap();
    attempt["binding_restorations"] = json!([{"operation_id":"restore","restored_at":1,"attempt_id":"current","executor_tab_id":"worker","session_id":"session","provider":"provider","owner_epoch":7,"old_nonce_digest":"old","current_nonce_digest":"current","lineage_proof_kind":"lineage","lineage_proof_digest":"digest"}]);
    attempt["continuation"] = json!({"epoch":1,"review_round":1,"state":"continued","milestone_id":"milestone","delivered_at":1});
    attempt["interruption"] = json!({"owner_epoch":7,"interrupted_at":1,"reason":"interruption","checkpoint_digest":"digest"});
    attempt["plan_skill_manifest"] =
        json!([{"slug":"skill","title":"Skill","purpose":"purpose","content_digest":"digest"}]);
    attempt["loaded_plan_skills"] =
        json!([{"slug":"skill","content_digest":"digest","loaded_at":1}]);
    attempt["lane_turn_stats"] = json!([{"turn":1,"captured_at":1,"transcript_turn_key":"turn","numstat":"stats","staged_numstat":"staged","untracked_file_count":1,"checkpoint_ref":"checkpoint","checkpoint_refusal":"deadline"}]);
    task["execution_attempts"] = json!([attempt]);
    task["milestones"] = json!([{"id":"milestone","from":{"tab_id":"worker","session_id":"session","title":"Worker"},"kind":"decision","text":"selected milestone","ts":1,"links":["link"],"loop_id":"loop","iteration":1,"canvas":{"x":1.0,"y":2.0,"collapsed":false},"attachments":[attachment]}]);
    plan.tasks[0] = serde_json::from_value(task).unwrap();
    plan
}

fn alternatives(value: &Value) -> Vec<Value> {
    let mut candidates = vec![
        Value::Null,
        json!("changed"),
        json!(0),
        json!(1),
        json!(true),
        json!(false),
        json!([]),
        json!({}),
    ];
    for spelling in [
        "pending",
        "in_progress",
        "review",
        "qa",
        "queued",
        "running",
        "passed",
        "failed",
        "p0",
        "p1",
        "p2",
        "p3",
        "user_supplied",
        "suggested",
        "repository_mutation",
        "non_repository",
        "isolated_lane",
        "canonical_writer",
        "shared",
        "read_only",
        "unspecified",
        "mutating",
    ] {
        candidates.push(json!(spelling));
    }
    match value {
        Value::String(text) => candidates.push(json!(format!("{text}-changed"))),
        Value::Number(number) => {
            if let Some(number) = number.as_u64() {
                candidates.push(json!(number + 1));
            } else if let Some(number) = number.as_f64() {
                candidates.push(json!(number + 1.0));
            }
        }
        Value::Bool(value) => candidates.push(json!(!value)),
        Value::Array(values) => {
            let mut changed = values.clone();
            changed.push(json!("changed"));
            candidates.push(json!(changed));
        }
        Value::Object(fields) => {
            for (name, field) in fields {
                for changed in alternatives(field) {
                    let mut object = fields.clone();
                    object.insert(name.clone(), changed);
                    candidates.push(Value::Object(object));
                }
            }
        }
        Value::Null => (),
    }
    candidates
}

fn changed_field<T: DeserializeOwned + Serialize + PartialEq>(row: &T, field: &str) -> T {
    let value = serde_json::to_value(row).unwrap();
    for alternative in alternatives(&value[field]) {
        let mut changed = value.clone();
        changed[field] = alternative;
        if let Ok(changed) = serde_json::from_value::<T>(changed) {
            if &changed != row {
                return changed;
            }
        }
    }
    panic!("no semantic mutation for field {field}");
}

#[test]
fn continuity_covers_every_serialized_metadata_milestone_and_attempt_field() {
    let directory = tempfile::tempdir().unwrap();
    let plan = rich_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let baseline = database
        .task_continuity_projection(&plan.tasks[0].id)
        .unwrap()
        .digest;
    for field in serde_json::to_value(&plan.tasks[0])
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
    {
        if matches!(field.as_str(), "milestones" | "execution_attempts") {
            continue;
        }
        let mut changed = plan.clone();
        changed.tasks[0] = changed_field::<Task>(&plan.tasks[0], field);
        database.persist_plan_rows(&changed, None).unwrap();
        assert_ne!(
            database
                .task_continuity_projection(&changed.tasks[0].id)
                .unwrap()
                .digest,
            baseline,
            "metadata {field}"
        );
    }
    for field in serde_json::to_value(&plan.tasks[0].milestones[0])
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
    {
        let mut changed = plan.clone();
        changed.tasks[0].milestones[0] =
            changed_field::<Milestone>(&plan.tasks[0].milestones[0], field);
        database.persist_plan_rows(&changed, None).unwrap();
        assert_ne!(
            database
                .task_continuity_projection(&changed.tasks[0].id)
                .unwrap()
                .digest,
            baseline,
            "milestone {field}"
        );
    }
    for field in serde_json::to_value(&plan.tasks[0].execution_attempts[0])
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
    {
        let mut changed = plan.clone();
        changed.tasks[0].execution_attempts[0] =
            changed_field::<TaskExecutionAttempt>(&plan.tasks[0].execution_attempts[0], field);
        database.persist_plan_rows(&changed, None).unwrap();
        assert_ne!(
            database
                .task_continuity_projection(&changed.tasks[0].id)
                .unwrap()
                .digest,
            baseline,
            "attempt {field}"
        );
    }
    for attempts in [false, true] {
        for operation in ["insert", "delete", "reorder"] {
            let mut changed = plan.clone();
            if attempts {
                let mut other = changed.tasks[0].execution_attempts[0].clone();
                other.id = "other".into();
                changed.tasks[0].execution_attempts.push(other);
            } else {
                let mut other = changed.tasks[0].milestones[0].clone();
                other.id = "other".into();
                changed.tasks[0].milestones.push(other);
            }
            database.persist_plan_rows(&changed, None).unwrap();
            let before = database
                .task_continuity_projection(&changed.tasks[0].id)
                .unwrap()
                .digest;
            if attempts {
                match operation {
                    "insert" => {
                        let mut row = changed.tasks[0].execution_attempts[0].clone();
                        row.id = "inserted".into();
                        changed.tasks[0].execution_attempts.push(row);
                    }
                    "delete" => {
                        changed.tasks[0].execution_attempts.pop();
                    }
                    _ => changed.tasks[0].execution_attempts.swap(0, 1),
                }
            } else {
                match operation {
                    "insert" => {
                        let mut row = changed.tasks[0].milestones[0].clone();
                        row.id = "inserted".into();
                        changed.tasks[0].milestones.push(row);
                    }
                    "delete" => {
                        changed.tasks[0].milestones.pop();
                    }
                    _ => changed.tasks[0].milestones.swap(0, 1),
                }
            }
            database.persist_plan_rows(&changed, None).unwrap();
            assert_ne!(
                database
                    .task_continuity_projection(&changed.tasks[0].id)
                    .unwrap()
                    .digest,
                before,
                "attempts={attempts} operation={operation}"
            );
        }
    }
}

#[test]
fn continuity_preserves_semantic_noops_defaults_key_order_and_signed_zero() {
    let directory = tempfile::tempdir().unwrap();
    let mut plan = authority_plan(directory.path());
    plan.tasks[0].canvas.x = Some(0.0);
    plan.tasks[0].key = None;
    let database = fixtures::database(directory.path(), &plan);
    let before = database.task_continuity_projection("task_exact").unwrap();
    database.persist_plan_rows(&plan, None).unwrap();
    assert_eq!(
        database.task_continuity_projection("task_exact").unwrap(),
        before
    );
    database.connection.lock().execute("UPDATE plan_tasks SET row_json=json_set(json_remove(row_json,'$.body','$.review_round','$.execution_intent','$.lane'),'$.speed',NULL,'$.key',NULL,'$.canvas.x',-0.0,'$.ignored_continuity_field','opaque') WHERE task_id='task_exact'",[]).unwrap();
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(json_remove(row_json,'$.winner','$.provider','$.assigned_at'),'$.binding_restorations',json('[]')) WHERE task_id='task_exact'",[]).unwrap();
    assert_eq!(
        database.task_continuity_projection("task_exact").unwrap(),
        before
    );
    let raw: String = database
        .connection
        .lock()
        .query_row(
            "SELECT row_json FROM plan_tasks WHERE task_id='task_exact'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let value: Value = serde_json::from_str(&raw).unwrap();
    let mut fields: Vec<_> = value.as_object().unwrap().iter().collect();
    fields.reverse();
    let reordered = format!(
        "{{ {} }}",
        fields
            .into_iter()
            .map(|(key, value)| format!(
                "{} : {}",
                serde_json::to_string(key).unwrap(),
                serde_json::to_string_pretty(value).unwrap()
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );
    database
        .connection
        .lock()
        .execute(
            "UPDATE plan_tasks SET row_json=?1 WHERE task_id='task_exact'",
            [reordered],
        )
        .unwrap();
    assert_eq!(
        database.task_continuity_projection("task_exact").unwrap(),
        before
    );
}

#[test]
fn continuity_detects_a_commit_between_capture_and_revalidation() {
    let directory = tempfile::tempdir().unwrap();
    let plan = authority_plan(directory.path());
    let database = fixtures::database(directory.path(), &plan);
    let before = database.task_continuity_projection("task_exact").unwrap();
    let path = database.path();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = barrier.clone();
    let writer = std::thread::spawn(move || {
        let mut connection = rusqlite::Connection::open(path).unwrap();
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        let tx = connection.transaction().unwrap();
        tx.execute("UPDATE plan_tasks SET row_json=json_set(row_json,'$.body','committed change') WHERE task_id='task_exact'",[]).unwrap();
        writer_barrier.wait();
        tx.commit().unwrap();
        writer_barrier.wait();
    });
    assert_eq!(
        database.task_continuity_projection("task_exact").unwrap(),
        before
    );
    barrier.wait();
    barrier.wait();
    assert_ne!(
        database
            .task_continuity_projection("task_exact")
            .unwrap()
            .digest,
        before.digest
    );
    writer.join().unwrap();
    database.connection.lock().execute("UPDATE plan_task_attempts SET row_json=json_set(row_json,'$.acknowledged_at','invalid') WHERE task_id='task_exact'",[]).unwrap();
    assert!(database.task_continuity_projection("task_exact").is_err());
}

#[test]
fn continuity_returns_fixed_bytes_and_reports_selected_row_scan_cost() {
    let mut prior = None;
    for (strides, attempts, unrelated_body, target_body, history) in [
        (0, 1, 0, 0, 0),
        (2048, 1, 0, 0, 0),
        (0, 2048, 0, 0, 0),
        (0, 1, 32768, 0, 0),
        (0, 1, 0, 32768, 0),
        (0, 1, 0, 0, 2048),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let mut plan = fixtures::fixture(directory.path(), strides, attempts);
        plan.tasks[1].body.push_str(&"x".repeat(unrelated_body));
        plan.tasks[0].body.push_str(&"x".repeat(target_body));
        let source = plan.tasks[0].execution_attempts[0].clone();
        for index in 0..history {
            let mut attempt = source.clone();
            attempt.id = format!("closed-{index}");
            attempt.closed_at = Some(2);
            plan.tasks[0].execution_attempts.push(attempt);
        }
        let database = fixtures::database(directory.path(), &plan);
        fixtures::audit(&database);
        take_metrics();
        crate::service_plan::store_sync::take_decoded_row_bytes();
        crate::service_plan::store_rows::take_serialized_row_bytes();
        let read = database.task_continuity_projection("task_0000").unwrap();
        let metrics = take_metrics();
        let decoded = crate::service_plan::store_sync::take_decoded_row_bytes();
        let serialized = crate::service_plan::store_rows::take_serialized_row_bytes();
        let response = serde_json::to_vec(&read).unwrap().len();
        assert_eq!(
            serde_json::to_value(&read.digest)
                .unwrap()
                .as_str()
                .unwrap()
                .len(),
            64
        );
        assert_eq!(metrics.queries, 1);
        assert!(metrics.materialized_bytes < 256);
        assert_eq!(serialized, 0);
        assert!(fixtures::audit_counts(&database).is_empty());
        let scans: (usize,usize) = database.connection.lock().query_row(
            "SELECT 1+(SELECT count(*) FROM plan_task_milestones WHERE plan_id=?1 AND task_id='task_0000')+(SELECT count(*) FROM plan_task_attempts WHERE plan_id=?1 AND task_id='task_0000'),
                length(json_remove(row_json,'$.milestones','$.execution_attempts'))+coalesce((SELECT sum(length(row_json)) FROM plan_task_milestones WHERE plan_id=?1 AND task_id='task_0000'),0)+coalesce((SELECT sum(length(row_json)) FROM plan_task_attempts WHERE plan_id=?1 AND task_id='task_0000'),0)
             FROM plan_tasks WHERE plan_id=?1 AND task_id='task_0000'",[&plan.id],|row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(
            decoded,
            scans.1 + serde_json::to_string(&read.status).unwrap().len()
        );
        println!("fixture={} consumer=task_continuity tasks={} base_bytes={} old_strides={strides} unrelated_attempts={attempts} unrelated_body={unrelated_body} target_body={target_body} target_history={history} queries={} materialized_text_bytes={} scanned_rows={} scanned_json_bytes={} decoded_json_bytes={decoded} serialized_row_bytes={serialized} response_bytes={response} changed_rows=0",fixtures::SEED,fixtures::TASKS,fixtures::CONTENT_BYTES,metrics.queries,metrics.materialized_bytes,scans.0,scans.1);
        if let Some((prior_metrics, prior_response, prior_digest, prior_decoded)) = &prior {
            assert_eq!(&metrics, prior_metrics);
            assert_eq!(&response, prior_response);
            if target_body == 0 && history == 0 {
                assert_eq!(&read.digest, prior_digest);
                assert_eq!(&decoded, prior_decoded);
            } else {
                assert_ne!(&read.digest, prior_digest);
                assert!(decoded > *prior_decoded);
            }
        } else {
            prior = Some((metrics, response, read.digest, decoded));
        }
    }
}
