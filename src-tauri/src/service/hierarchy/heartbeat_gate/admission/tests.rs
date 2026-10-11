use super::*;
use serde_json::json;

#[test]
fn sixty_churning_minute_ticks_admit_at_most_two_judgements() {
    for interval in [1800, 2400, 2700] {
        let mut admission = Admission::default();
        let mut calls = 0;
        for minute in 0..60 {
            let snapshot = Snapshot {
                projection: [("task:task".into(), minute.to_string())].into(),
                hard: vec![HardWake::TaskState("task".into())],
                context: [("task:task".into(), json!({"status": "in_progress"}))].into(),
                ..Default::default()
            };
            let (next, result) = admit(&admission, &snapshot, false, minute * 60, interval);
            admission = next;
            calls += usize::from(result.is_ok());
        }
        assert_eq!(calls, 2);
    }
}

#[test]
fn actionable_events_admit_exactly_once_despite_snapshot_churn() {
    for event in [
        HardWake::NewReport("report".into()),
        HardWake::UndeliveredReport("report".into()),
        HardWake::TaskClosed("task".into()),
        HardWake::Incident("worker_health:worker".into()),
        HardWake::FailedGate("gate".into()),
        HardWake::OwnerMessage("message".into()),
        HardWake::ChoiceAnswer("choice".into()),
    ] {
        let (mut admission, _) = admit(&Admission::default(), &Snapshot::default(), false, 0, 1800);
        let mut calls = 0;
        for minute in 1..10 {
            let snapshot = Snapshot {
                hard: vec![event.clone()],
                context: [("task:task".into(), json!({"status": "done"}))].into(),
                ..Default::default()
            };
            let (next, result) = admit(&admission, &snapshot, false, minute * 60, 1800);
            admission = next;
            calls += usize::from(result.is_ok());
            assert_eq!(result.is_ok(), minute == 1);
        }
        assert_eq!(calls, 1);
    }
}

#[test]
fn identical_snapshot_never_admits_even_after_cadence() {
    let (admission, _) = admit(&Admission::default(), &Snapshot::default(), false, 0, 1800);
    let (_, result) = admit(&admission, &Snapshot::default(), true, 3600, 1800);
    assert_eq!(result, Err("same_snapshot"));
}

#[test]
fn new_prompt_preserves_immediate_safety_admission() {
    let (admission, _) = admit(&Admission::default(), &Snapshot::default(), false, 0, 1800);
    let snapshot = Snapshot {
        workers: vec![super::super::Worker {
            tab_id: "worker".into(),
            prompt: Some("manual_update".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let (admission, result) = admit(&admission, &snapshot, false, 60, 1800);
    assert!(result.is_ok());
    assert_eq!(
        admit(&admission, &snapshot, false, 120, 1800).1,
        Err("judgement_cadence")
    );
}

#[test]
fn task_events_admit_closure_and_blockage_without_status_churn() {
    use super::super::snapshot::task_wake;
    use crate::plan::TaskStatus;

    for (status, immediate) in [
        (TaskStatus::Done, true),
        (TaskStatus::Cancelled, true),
        (TaskStatus::Blocked, true),
        (TaskStatus::Pending, false),
        (TaskStatus::InProgress, false),
        (TaskStatus::InReview, false),
        (TaskStatus::Paused, false),
    ] {
        let (admission, _) = admit(&Admission::default(), &Snapshot::default(), false, 0, 1800);
        let snapshot = Snapshot {
            hard: vec![task_wake("task", &status)],
            ..Default::default()
        };
        assert_eq!(
            admit(&admission, &snapshot, false, 60, 1800).1.is_ok(),
            immediate
        );
    }
}
