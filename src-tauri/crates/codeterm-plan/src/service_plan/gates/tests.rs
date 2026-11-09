use super::*;
use crate::plan::{OrchestratorRef, Plan};
use crate::service_plan::{NewTask, PlanStore};
use tempfile::TempDir;

fn store_with_tasks(names: &[&str]) -> (TempDir, PlanStore, String, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Gate test".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch_tab"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_ids = names
        .iter()
        .map(|n| {
            store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: (*n).into(),
                        kind: "implement".into(),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap()
        })
        .collect();
    (dir, store, plan.id, task_ids)
}

#[test]
fn task_pause_status_preservation_gate_hold() {
    let (_dir, store, pid, tids) = store_with_tasks(&["task-one", "task-two", "task-three"]);
    let (t1, t2, t3) = (tids[0].clone(), tids[1].clone(), tids[2].clone());

    let gate = store
        .ask_gate(
            &pid,
            "Which direction?",
            "",
            &[t1.clone()],
            None,
            Some("orch_tab"),
            1000,
        )
        .unwrap();
    assert!(gate.is_open());
    assert_eq!(gate.blocking_task_ids, vec![t1.clone()]);

    let plan = store.get(&pid).unwrap();
    let task1 = plan.tasks.iter().find(|t| t.id == t1).unwrap();
    assert_eq!(task1.status, TaskStatus::Paused);
    assert!(task1
        .pause_reason
        .as_deref()
        .unwrap()
        .starts_with("gate:gate_"));

    let task2 = plan.tasks.iter().find(|t| t.id == t2).unwrap();
    let task3 = plan.tasks.iter().find(|t| t.id == t3).unwrap();
    assert_eq!(task2.status, TaskStatus::Pending);
    assert_eq!(task3.status, TaskStatus::Pending);

    assert!(plan.tasks.iter().any(|t| t.status.counts_as_open_work()));
}

#[test]
fn review_hold_verdict_gate_guard_survives() {
    let (_dir, store, plan_id, task_ids) = store_with_tasks(&["reviewed-task"]);
    let mut plan = store.get(&plan_id).unwrap();
    plan.tasks[0].status = TaskStatus::InReview;
    plan.review_policy.credit = -1;
    store.save(&mut plan, 3).unwrap();

    let error = store
        .ask_gate(
            &plan_id,
            "Pause review?",
            "",
            &task_ids,
            None,
            Some("orch_tab"),
            4,
        )
        .unwrap_err();
    assert!(matches!(error, PlanError::TransitionDenied { .. }));
    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.tasks[0].status, TaskStatus::InReview);
    assert_eq!(plan.review_policy.credit, -1);
    assert!(plan.gates.is_empty());
}

#[test]
fn gate_and_durable_request_share_one_plan_mutation() {
    let (_dir, store, plan_id, task_ids) = store_with_tasks(&["blocked-task"]);
    let (gate, request) = store
        .ask_gate_with_planner_request(
            &plan_id,
            "Audit the model?",
            "- preserve `ticks`\n- reject <nonexistent>",
            &task_ids,
            Some("planner".into()),
            Some("caller"),
            Some("caller-session"),
            1000,
        )
        .unwrap();

    let plan = store.get(&plan_id).unwrap();
    assert_eq!(plan.gates.len(), 1);
    assert_eq!(plan.planner_requests.len(), 1);
    assert_eq!(plan.planner_inbox.len(), 1);
    assert_eq!(request.request_id, format!("gate:{}", gate.id));
    assert_eq!(plan.planner_requests[0], request);
    assert_eq!(plan.planner_inbox[0].request_id, request.request_id);
    assert!(request.payload.contains("- preserve `ticks`"));
    assert_eq!(plan.tasks[0].status, TaskStatus::Paused);
}

#[test]
fn answer_gate_resumes_tasks_and_is_immutable() {
    let (_dir, store, pid, tids) = store_with_tasks(&["blocked-task"]);
    let t1 = tids[0].clone();

    let gate = store
        .ask_gate(&pid, "Decision?", "", &[t1.clone()], None, None, 1000)
        .unwrap();
    let gid = gate.id.clone();

    let answered = store
        .answer_gate(&pid, &gid, "Go left.", Some("planner_tab"), 2000)
        .unwrap();
    assert_eq!(answered.status, GateStatus::Answered);
    assert_eq!(answered.answer.as_ref().unwrap().text, "Go left.");

    let plan = store.get(&pid).unwrap();
    let task1 = plan.tasks.iter().find(|t| t.id == t1).unwrap();
    assert_eq!(
        task1.status,
        TaskStatus::Pending,
        "task must be resumed after answer"
    );

    // Second answer must fail (immutable audit record)
    let err = store
        .answer_gate(&pid, &gid, "Nope.", None, 3000)
        .unwrap_err();
    assert!(matches!(err, PlanError::GateAlreadyAnswered));
}

#[test]
fn empty_question_is_rejected() {
    let (_dir, store, pid, _) = store_with_tasks(&[]);
    let err = store
        .ask_gate(&pid, "  ", "", &[], None, None, 1000)
        .unwrap_err();
    assert!(matches!(err, PlanError::GateRequiresQuestion));
}

#[test]
fn plan_never_fully_frozen_by_one_gate() {
    let (_dir, store, pid, tids) = store_with_tasks(&["blocked", "free"]);
    store
        .ask_gate(&pid, "Question?", "", &[tids[0].clone()], None, None, 1000)
        .unwrap();
    let plan = store.get(&pid).unwrap();
    assert!(
        plan.tasks.iter().any(|t| t.status.counts_as_open_work()),
        "plan must retain open work so frontier keeps dispatching"
    );
}

#[test]
fn gate_doctor_flags_paused_task_without_reason() {
    let mut plan = Plan::new("T".into(), "/p".into(), OrchestratorRef::from_tab("p"));
    plan.tasks.push(crate::plan::Task {
        files: Default::default(),
        id: "task_x".into(),
        name: "x".into(),
        kind: "implement".into(),
        status: TaskStatus::Paused,
        pause_reason: None,
        personality: None,
        personality_provenance: None,
        model: None,
        speed: None,
        model_rationale: None,
        acceptance_criteria: vec![],
        external_refs: vec![],
        test_plan: None,
        qa_plan: None,
        correctness: None,
        review_stage: Default::default(),
        qa_state: None,
        review_round: 0,
        review_oracle: None,
        review_evidence: vec![],
        key: None,
        review_override: None,
        review_audit: vec![],
        status_label: None,
        priority: Default::default(),
        created_at: 0,
        updated_at: 0,
        status_changed_at: 0,
        handle: None,
        tags: vec![],
        section_ids: vec![],
        body: String::new(),
        role_guidance: None,
        executor_role: None,
        competition: None,
        intent: None,
        execution_intent: Default::default(),
        parent_task_id: None,
        depends_on: vec![],
        assigned_tab_id: None,
        lane: Default::default(),
        taken_by_role: None,
        peer: None,
        group: None,
        canvas: Default::default(),
        milestones: vec![],
        execution_attempts: vec![],
        interruption: Default::default(),
        attachments: vec![],
    });
    let findings = gate_doctor_findings(&plan, 1_000);
    assert!(findings
        .iter()
        .any(|f| f.kind == "paused_task_missing_reason" && f.severity == "error"));
}

#[test]
fn gate_doctor_warns_on_stale_open_gate() {
    let mut plan = Plan::new("T".into(), "/p".into(), Default::default());
    plan.gates.push(DecisionGate {
        id: "gate_stale".into(),
        question: "Stale question?".into(),
        context: String::new(),
        blocking_task_ids: vec![],
        routed_to_tab_id: None,
        asked_by_tab_id: None,
        asked_at: 0,
        status: GateStatus::Open,
        answer: None,
        escalated_at: None,
        escalation_note: None,
    });
    let findings = gate_doctor_findings(&plan, STALE_GATE_MS + 1);
    assert!(findings
        .iter()
        .any(|f| f.kind == "stale_open_gate" && f.severity == "warning"));
}
