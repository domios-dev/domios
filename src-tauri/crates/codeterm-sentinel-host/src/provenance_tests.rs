use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use codeterm_decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionRequest, TokenMode,
};
use serde_json::Value;

use super::provenance::{Classification, Scope, Stage};
use super::store::Sentinel;
use super::{classify, machine, scheduler};
use codeterm_service_core::service::supporter::SupporterRuntime;

#[derive(Clone, Copy)]
enum Answer {
    Choice(&'static str, f64),
    Budget,
    Credit,
    Deadline,
    Error,
    Invalid,
}

struct Model {
    answer: Answer,
    calls: Arc<AtomicUsize>,
}

impl DecisionModel for Model {
    fn id(&self) -> &str {
        "opaque-adapter"
    }
    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: true,
            ..Default::default()
        }
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.answer {
            Answer::Choice(bucket, confidence) => Ok(DecisionAnswer::Choice {
                choice: bucket.into(),
                confidence: Some(confidence),
                probabilities: BTreeMap::new(),
            }),
            Answer::Budget => Err(DecisionError::Budget),
            Answer::Credit => Err(DecisionError::CreditExhausted),
            Answer::Deadline => Err(DecisionError::Deadline),
            Answer::Error => Err(DecisionError::Transport("opaque-error".into())),
            Answer::Invalid => Ok(DecisionAnswer::Noul { p: 0.95 }),
        }
    }
}

fn job(dir: &std::path::Path) -> Sentinel {
    let mut store = super::tests::store(
        dir,
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    super::tests::create(&mut store, "team_aabbccdd")
}

fn evaluate(
    job: &mut Sentinel,
    output: &[u8],
    answer: Answer,
    calls: Arc<AtomicUsize>,
) -> machine::Outcome {
    let request = classify::request(job, std::str::from_utf8(output).unwrap());
    let scope = Scope::from(&*job);
    machine::evaluate(
        &mut job.machine,
        output,
        &job.buckets,
        &job.routes,
        None,
        scope,
        || {
            classify::decide(
                Arc::new(Model { answer, calls }),
                &request,
                Instant::now() + Duration::from_secs(1),
            )
        },
    )
}

fn rows(dir: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("supporter/journal.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn sentinel_provenance_route_stages_share_observation_without_recipient_claim() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    job.routes.insert(
        "passed".into(),
        vec![super::handlers::Handler {
            id: "send".into(),
            action: machine::Route::Send("opaque-target".into()),
            target: None,
        }],
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = evaluate(
        &mut job,
        b"opaque-private-probe",
        Answer::Choice("passed", 0.95),
        calls.clone(),
    );
    for stage in [Stage::Decision, Stage::RouteClaim, Stage::RouteCompleted] {
        scheduler::record(&runtime, &job, &outcome, stage).unwrap();
    }
    let rows = rows(dir.path());
    assert_eq!(rows.len(), 3);
    let required_keys = [
        "time",
        "kind",
        "blueprint_id",
        "tab",
        "team_id",
        "workspace_id",
        "state_hash",
        "evaluation_id",
        "observation_id",
        "classification_source",
        "classification_reason",
        "label",
        "confidence",
        "action",
        "reason",
        "stage",
        "route_status",
        "route",
        "route_target",
        "requested_by",
        "recipient_receipt",
        "asked",
        "budget",
    ];
    let decision = rows[0].as_object().unwrap();
    for key in required_keys {
        assert!(decision.contains_key(key), "journal row lacks {key}");
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    for row in &rows {
        assert_eq!(row["evaluation_id"], rows[0]["evaluation_id"]);
        assert_eq!(row["observation_id"], rows[0]["observation_id"]);
        assert!(uuid::Uuid::parse_str(row["observation_id"].as_str().unwrap()).is_ok());
        assert_eq!(row["state_hash"], job.machine.last_hash.as_deref().unwrap());
        assert_eq!(row["tab"], "creator");
        assert_eq!(row["team_id"], "team_aabbccdd");
        assert_eq!(row["blueprint_id"], job.id);
        assert_eq!(row["workspace_id"], "workspace");
        assert_eq!(row["classification_source"], "adapter");
        assert_eq!(row["classification_reason"], "transition");
        assert_eq!(
            row["label"],
            codeterm_service_core::service::supporter::criteria_hash(b"passed")
        );
        assert_eq!(row["route_target"], "opaque-target");
        assert!(row["requested_by"].is_null());
        assert!(row["recipient_receipt"].is_null());
        assert!(row.get("probe_output").is_none());
        assert!(!row.to_string().contains("opaque-private-probe"));
    }
    assert_eq!(rows[0]["route_status"], "not_attempted");
    assert_eq!(rows[1]["stage"], "route_claim");
    assert_eq!(rows[1]["route_status"], "claimed");
    assert_eq!(rows[1]["action"], "route_claim");
    assert_eq!(rows[2]["stage"], "route_completed");
    assert_eq!(rows[2]["route_status"], "api_completed");
    assert_eq!(rows[2]["action"], "send");
}

#[test]
fn sentinel_provenance_evaluated_occurrences_have_distinct_ids() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    let calls = Arc::new(AtomicUsize::new(0));
    for output in [b"opaque-a", b"opaque-b", b"opaque-a"] {
        let outcome = evaluate(
            &mut job,
            output,
            Answer::Choice("passed", 0.95),
            calls.clone(),
        );
        scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    }
    let rows = rows(dir.path());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(rows[0]["state_hash"], rows[2]["state_hash"]);
    let ids: std::collections::BTreeSet<_> = rows
        .iter()
        .map(|row| row["observation_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(rows[1]["reason"], "same_bucket");
    assert_eq!(rows[2]["reason"], "same_bucket");
}

#[test]
fn sentinel_provenance_rejects_foreign_scope_or_state_hash() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    let outcome = evaluate(
        &mut job,
        b"opaque-a",
        Answer::Choice("passed", 0.95),
        Arc::new(AtomicUsize::new(0)),
    );
    for field in ["blueprint", "creator", "team", "workspace", "state_hash"] {
        let mut foreign = job.clone();
        match field {
            "blueprint" => foreign.id = "foreign-watch".into(),
            "creator" => foreign.creator_tab_id = "foreign-creator".into(),
            "team" => foreign.team_id = Some("team_11223344".into()),
            "workspace" => foreign.workspace_id = "foreign-workspace".into(),
            _ => foreign.machine.last_hash = Some("foreign-hash".into()),
        }
        for stage in [Stage::Decision, Stage::RouteClaim, Stage::RouteCompleted] {
            assert!(
                scheduler::record(&runtime, &foreign, &outcome, stage).is_err(),
                "{field}"
            );
        }
    }
    assert!(!dir.path().join("supporter/journal.jsonl").exists());
    scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    assert_eq!(rows(dir.path()).len(), 1);
}

#[test]
fn sentinel_provenance_regex_and_disconnection_keep_actual_source() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    job.matches.insert("passed".into(), "^opaque-match$".into());
    for output in ["opaque-match", "opaque-unmatched", "opaque-disconnected"] {
        if output == "opaque-disconnected" {
            job.matches.clear();
        }
        let classification = classify::fallback(&job, output);
        let scope = Scope::from(&job);
        let outcome = machine::evaluate(
            &mut job.machine,
            output.as_bytes(),
            &job.buckets,
            &job.routes,
            None,
            scope,
            || classification,
        );
        scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    }
    let rows = rows(dir.path());
    assert_eq!(rows[0]["classification_source"], "pattern");
    assert_eq!(rows[0]["classification_reason"], "transition");
    assert_eq!(rows[1]["classification_source"], "pattern");
    assert_eq!(rows[1]["classification_reason"], "other");
    assert!(rows[1]["route"].is_null());
    assert_eq!(rows[2]["classification_source"], "disconnected");
    assert_eq!(rows[2]["classification_reason"], "adapter_disconnected");
    assert!(rows[2]["observation_id"].is_null());
    assert!(rows[2]["label"].is_null());
}

#[test]
fn sentinel_provenance_journals_patterns_without_model_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    job.matches
        .insert("failed".into(), "^opaque-pattern$".into());
    for output in ["opaque-pattern", "opaque-unmatched"] {
        let classification = classify::with_pattern_precedence(&job, output, || {
            Classification::adapter(Ok(machine::Verdict {
                bucket: "passed".into(),
                confidence: 0.9,
            }))
        });
        let scope = Scope::from(&job);
        let outcome = machine::evaluate(
            &mut job.machine,
            output.as_bytes(),
            &job.buckets,
            &job.routes,
            None,
            scope,
            || classification,
        );
        scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    }

    let rows = rows(dir.path());
    assert_eq!(rows[0]["classification_source"], "pattern");
    assert_eq!(rows[1]["classification_source"], "pattern");
}

#[test]
fn sentinel_provenance_adapter_skips_and_route_failure_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    let calls = Arc::new(AtomicUsize::new(0));
    for (index, (answer, reason)) in [
        (Answer::Budget, "budget_exhausted"),
        (Answer::Credit, "credit_exhausted"),
        (Answer::Deadline, "model_deadline"),
        (Answer::Error, "model_failed"),
        (Answer::Invalid, "invalid_answer"),
        (Answer::Choice("other", 0.99), "other"),
        (Answer::Choice("passed", 0.79), "low_confidence"),
    ]
    .into_iter()
    .enumerate()
    {
        let outcome = evaluate(
            &mut job,
            format!("opaque-{index}").as_bytes(),
            answer,
            calls.clone(),
        );
        assert_eq!(outcome.reason, reason);
        scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
        assert!(scheduler::record(&runtime, &job, &outcome, Stage::RouteClaim).is_err());
    }
    let outcome = evaluate(
        &mut job,
        b"opaque-final",
        Answer::Choice("passed", 0.95),
        calls,
    );
    scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    scheduler::record(&runtime, &job, &outcome, Stage::RouteClaim).unwrap();
    scheduler::record(&runtime, &job, &outcome, Stage::RouteFailed).unwrap();
    let rows = rows(dir.path());
    for row in &rows[..7] {
        assert_eq!(row["classification_source"], "adapter");
        assert_eq!(row["route_status"], "not_attempted");
        assert!(row["route"].is_null());
        assert!(row["observation_id"].is_string());
    }
    assert_eq!(rows[9]["observation_id"], rows[7]["observation_id"]);
    assert_eq!(rows[9]["state_hash"], rows[7]["state_hash"]);
    assert_eq!(rows[9]["classification_reason"], "transition");
    assert_eq!(rows[9]["reason"], "route_failed");
    assert_eq!(rows[9]["route_status"], "failed");
    assert_eq!(rows[9]["action"], "skip");
    assert!(rows[9]["recipient_receipt"].is_null());
    assert!(!rows.iter().any(|row| row["stage"] == "route_completed"));
}

#[test]
fn sentinel_provenance_unchanged_is_not_a_new_classification() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    let calls = Arc::new(AtomicUsize::new(0));
    let first = evaluate(
        &mut job,
        b"opaque-fixed",
        Answer::Choice("passed", 0.95),
        calls.clone(),
    );
    scheduler::record(&runtime, &job, &first, Stage::Decision).unwrap();
    let scope = Scope::from(&job);
    let unchanged = machine::evaluate(
        &mut job.machine,
        b"opaque-fixed",
        &job.buckets,
        &job.routes,
        None,
        scope,
        || panic!("unchanged probe must not classify"),
    );
    scheduler::record(&runtime, &job, &unchanged, Stage::Decision).unwrap();
    let rows = rows(dir.path());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(rows[1]["reason"], "unchanged");
    assert_eq!(rows[1]["classification_source"], "not_attempted");
    assert!(rows[1]["observation_id"].is_null());
    assert!(rows[1]["label"].is_null());
    assert!(rows[1]["route"].is_null());
    assert_eq!(rows[0]["state_hash"], rows[1]["state_hash"]);
    assert_ne!(rows[0]["evaluation_id"], rows[1]["evaluation_id"]);
    assert!(scheduler::record(&runtime, &job, &unchanged, Stage::RouteClaim).is_err());
    let restored = SupporterRuntime::new(dir.path().join("supporter"));
    assert_eq!(restored.feed(10, Some("team_aabbccdd")).unwrap().len(), 2);
}

#[test]
fn sentinel_provenance_probe_failure_has_no_observation_or_stale_hash() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    let prior = evaluate(
        &mut job,
        b"opaque-prior",
        Answer::Choice("passed", 0.95),
        Arc::new(AtomicUsize::new(0)),
    );
    scheduler::record(&runtime, &job, &prior, Stage::Decision).unwrap();
    let failed = machine::Outcome {
        evidence: super::provenance::Evidence::new(Scope::from(&job), None),
        reason: "probe_timeout",
        verdict: None,
        route: None,
        handlers: Vec::new(),
    };
    scheduler::record(&runtime, &job, &failed, Stage::Skip).unwrap();
    let rows = rows(dir.path());
    assert_eq!(rows[1]["classification_source"], "not_attempted");
    assert_eq!(rows[1]["reason"], "probe_timeout");
    assert!(rows[1]["state_hash"].is_null());
    assert!(rows[1]["observation_id"].is_null());
    assert!(rows[1]["label"].is_null());
    assert!(rows[1]["route"].is_null());
    assert_ne!(rows[0]["evaluation_id"], rows[1]["evaluation_id"]);
}

#[test]
fn deterministic_probe_ticks_never_write_supporter_journal() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut job = job(dir.path());
    job.model_classification = false;
    job.matches.insert("passed".into(), "^passed$".into());
    for tick in 0..10 {
        let text = format!("unmatched-{tick}");
        let classification =
            classify::with_pattern_precedence(&job, &text, || panic!("no model calls"));
        let scope = Scope::from(&job);
        let outcome = machine::evaluate(
            &mut job.machine,
            text.as_bytes(),
            &job.buckets,
            &job.routes,
            None,
            scope,
            || classification,
        );
        scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    }
    assert!(!dir.path().join("supporter/journal.jsonl").exists());
}
