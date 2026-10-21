//! Review entry has exactly one gate left: the acceptance-oracle scope gate.
//! Review depth is advisory, so a task past the round bound enters review like
//! any other — these tests pin both halves so neither drifts back.
//!
//! Everything here asserts on `PlanError` variants and their `data.type`
//! constants. Never on the refusal's prose, and never on the remedy's wording.

use super::review_gate::{oracle_signature, validate_enter_review};
use super::{NewTask, PlanError, PlanStore};
use crate::plan::{
    AgentRef, OrchestratorRef, Plan, ReviewEvidence, ReviewOverride, ReviewVerdict, Task,
    DEFAULT_MAX_REVIEW_ROUNDS,
};

/// A task that has entered review once, so it carries an oracle and evidence.
/// `review_round` and the last bounce's flags are what each test then varies.
fn reviewed_task(dir: &std::path::Path) -> Task {
    let store = PlanStore::new_for_test(dir);
    let mut plan = Plan::new(
        "Review gate selection".into(),
        dir.display().to_string(),
        OrchestratorRef::from_tab("owner"),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Under review".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.test_plan = Some("./build.sh test-one plan the_original_oracle".into());
    task.acceptance_criteria = vec!["the original criterion".into()];
    let signature = oracle_signature(task);
    task.review_oracle = Some(signature);
    task.clone()
}

fn bounce(distinct_oracle: bool) -> ReviewEvidence {
    ReviewEvidence {
        id: "rev_1".into(),
        from: AgentRef::from_tab("reviewer"),
        verdict: ReviewVerdict::Bounce,
        note: "bounced".into(),
        at: 10,
        changed_executable_surfaces: false,
        distinct_oracle,
    }
}

/// The correction the bounce asked for, which moves the oracle signature.
fn correct_the_oracle(task: &mut Task) {
    task.test_plan = Some("./build.sh test-one plan the_corrected_oracle".into());
    task.acceptance_criteria = vec!["the corrected criterion".into()];
    let moved = oracle_signature(task);
    assert_ne!(
        task.review_oracle.as_deref(),
        Some(moved.as_str()),
        "the fixture must actually move the oracle, or neither gate is exercised"
    );
}

fn complete_override() -> ReviewOverride {
    ReviewOverride {
        owner: "andrey".into(),
        reason: "release-impacting".into(),
        reproduction: "steps".into(),
        at: 20,
    }
}

/// Past the round bound, review entry is no longer refused at all: the bound is
/// advice the Orchestrator acts on, never a gate the Worker has to clear.
#[test]
fn a_bounce_correction_past_the_round_limit_enters_review_unrefused() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = reviewed_task(dir.path());
    task.review_round = DEFAULT_MAX_REVIEW_ROUNDS;
    task.review_evidence = vec![bounce(true)];
    correct_the_oracle(&mut task);

    validate_enter_review(&task)
        .expect("the round bound is advisory; a compliant correction still enters review");
    assert!(
        task.review_override.is_none(),
        "and it enters with no override recorded, so nothing hides a gate"
    );
}

/// The other gate, on its own facts: the oracle moved with no bounce
/// authorizing it, below the round bound.
#[test]
fn an_unauthorized_oracle_change_reports_the_scope_gate() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = reviewed_task(dir.path());
    task.review_round = 1;
    assert!(
        task.review_round < DEFAULT_MAX_REVIEW_ROUNDS,
        "this case must sit below the round bound or it proves nothing"
    );
    task.review_evidence = vec![bounce(false)];
    correct_the_oracle(&mut task);

    let error = validate_enter_review(&task).unwrap_err();
    assert!(matches!(error, PlanError::ReviewOracleScopeExpansion));
    assert_eq!(error.to_json()["type"], "review_oracle_scope_expansion");
}

/// A bounce recorded with `distinct_oracle` is the reviewer authorizing the
/// change, so answering it is compliance and must not be refused at all.
#[test]
fn a_bounce_that_authorized_a_distinct_oracle_admits_its_own_correction() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = reviewed_task(dir.path());
    task.review_round = 1;
    task.review_evidence = vec![bounce(true)];
    correct_the_oracle(&mut task);

    validate_enter_review(&task)
        .expect("a correction the bounce asked for is not a scope expansion");
}

/// The scope gate still bites when nothing authorized the change, and an
/// unchanged oracle is never refused. Both directions, so the fix above cannot
/// be read as "the scope gate was removed".
#[test]
fn the_scope_gate_still_refuses_and_still_admits_an_unchanged_oracle() {
    let dir = tempfile::tempdir().unwrap();
    let mut unchanged = reviewed_task(dir.path());
    unchanged.review_round = 1;
    unchanged.review_evidence = vec![bounce(false)];
    validate_enter_review(&unchanged).expect("an unchanged oracle enters review freely");

    let mut widened = unchanged.clone();
    correct_the_oracle(&mut widened);
    assert!(matches!(
        validate_enter_review(&widened).unwrap_err(),
        PlanError::ReviewOracleScopeExpansion
    ));
}

/// An override clears the scope gate at any review depth, and an INCOMPLETE one
/// is named as incomplete rather than reported as scope expansion.
#[test]
fn an_override_clears_the_gate_that_fired_and_incompleteness_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let base = reviewed_task(dir.path());

    for round in [1, DEFAULT_MAX_REVIEW_ROUNDS] {
        let mut task = base.clone();
        task.review_round = round;
        task.review_evidence = vec![bounce(false)];
        correct_the_oracle(&mut task);

        task.review_override = Some(complete_override());
        validate_enter_review(&task)
            .unwrap_or_else(|error| panic!("a complete override clears round {round}: {error:?}"));

        task.review_override = Some(ReviewOverride {
            reproduction: String::new(),
            ..complete_override()
        });
        let error = validate_enter_review(&task).unwrap_err();
        assert!(
            matches!(error, PlanError::ReviewOverrideIncomplete),
            "round {round} must name the incomplete override, not the gate behind it"
        );
        assert_eq!(error.to_json()["type"], "review_override_incomplete");
    }
}

/// A scope override is CONSUMED when it clears the scope gate, and is never
/// written to the audit log: `ReviewAuditEntry` records review-ROUND-LIMIT
/// exceptions, which no longer exist.
#[test]
fn a_scope_override_is_consumed_below_the_bound_without_touching_the_audit_log() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = reviewed_task(dir.path());
    task.review_round = 1;
    task.review_evidence = vec![bounce(false)];
    correct_the_oracle(&mut task);
    task.review_override = Some(complete_override());

    super::review_gate::fix_oracle_and_consume_override(&mut task, 30);

    assert!(
        task.review_override.is_none(),
        "an override spent on the scope gate must not survive a later entry"
    );
    assert!(
        task.review_audit.is_empty(),
        "the round-limit audit log records round-limit exceptions only"
    );
    let fixed = oracle_signature(&task);
    assert_eq!(
        task.review_oracle.as_deref(),
        Some(fixed.as_str()),
        "the authorized correction fixes the new oracle"
    );
}

/// The round bound no longer spends anything: with no oracle change there is no
/// gate to clear, so an override recorded at the bound is neither burnt nor
/// audited. The audit log stays empty because round-limit exceptions are gone.
#[test]
fn a_round_bound_override_is_never_spent_or_audited() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = reviewed_task(dir.path());
    task.review_round = DEFAULT_MAX_REVIEW_ROUNDS;
    task.review_evidence = vec![bounce(false)];
    task.review_override = Some(complete_override());

    super::review_gate::fix_oracle_and_consume_override(&mut task, 30);

    assert_eq!(task.review_override, Some(complete_override()));
    assert!(task.review_audit.is_empty());
}

/// Nothing tripped, nothing spent: an ordinary first entry must not burn an
/// override that was recorded for a later round.
#[test]
fn an_unspent_override_survives_when_no_gate_fired() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = reviewed_task(dir.path());
    task.review_round = 1;
    task.review_evidence = vec![bounce(false)];
    task.review_override = Some(complete_override());

    super::review_gate::fix_oracle_and_consume_override(&mut task, 30);

    assert!(task.review_override.is_some());
    assert!(task.review_audit.is_empty());
}
