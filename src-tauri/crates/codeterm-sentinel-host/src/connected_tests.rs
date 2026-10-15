use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use super::provenance::{Classification, Scope, Stage};
use super::{classify, machine, scheduler};
use codeterm_decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionQuestion,
    DecisionRequest, TokenMode,
};
use codeterm_service_core::service::supporter::{self, SupporterRuntime, VerdictKind};

struct Model {
    calls: Arc<AtomicUsize>,
}

impl DecisionModel for Model {
    fn id(&self) -> &str {
        "fixture"
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
    fn decide(
        &self,
        request: &DecisionRequest,
        _: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        codeterm_decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        let DecisionQuestion::Choice { options } = &request.question else {
            panic!("must use Choice")
        };
        assert_eq!(
            options.keys().cloned().collect::<Vec<_>>(),
            ["failed", "other", "passed", "waiting"]
        );
        Ok(DecisionAnswer::Choice {
            choice: "passed".into(),
            confidence: Some(0.9),
            probabilities: BTreeMap::from([("passed".into(), 0.9)]),
        })
    }
}

#[test]
fn sentinel_connected_hash_gate_shared_budget_and_journal() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = Arc::new(SupporterRuntime::new(dir.path().join("supporter")));
    let model = supporter::budgeted_model_with_activity(
        Arc::new(Model {
            calls: calls.clone(),
        }),
        runtime.clone(),
        1,
        VerdictKind::Sentinel,
        Some("creator".into()),
        None,
        None,
    );
    let mut store = super::tests::store(
        dir.path(),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let mut job = super::tests::create(&mut store, "team");
    for (index, output) in ["private probe", "private probe", "changed private probe"]
        .iter()
        .enumerate()
    {
        let request = classify::request(&job, output);
        let scope = Scope::from(&job);
        let outcome = machine::evaluate(
            &mut job.machine,
            output.as_bytes(),
            &job.buckets,
            &job.routes,
            None,
            scope,
            || {
                classify::decide(
                    model.clone(),
                    &request,
                    Instant::now() + Duration::from_secs(1),
                )
            },
        );
        assert_eq!(
            outcome.route,
            if index == 0 {
                Some(machine::Route::Notify)
            } else {
                None
            }
        );
        assert_eq!(
            outcome.reason,
            ["transition", "unchanged", "budget_exhausted"][index]
        );
        scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let journal = std::fs::read_to_string(dir.path().join("supporter/journal.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        entries.iter().filter(|v| v["kind"] == "model_call").count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|v| v["reason"] == "budget_exhausted" && v["kind"] == "blueprint")
            .count(),
        1
    );
    assert!(!journal.contains("private probe"));
    let decision = entries
        .iter()
        .find(|entry| entry["action"] == "decision" && entry["reason"] == "transition")
        .unwrap();
    assert_eq!(decision["asked"].as_array().unwrap().len(), 1);
    assert!(decision["asked"][0]["question"]
        .as_str()
        .unwrap()
        .contains(&job.id));
    assert_eq!(decision["asked"][0]["answer"], "passed");
    assert_eq!(runtime.status(1)["budget"]["reached"], true);
}

#[test]
fn sentinel_feed_scrubs_job_name_and_declared_bucket_without_probe_text() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().join("supporter"));
    let mut store = super::tests::store(
        dir.path(),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let mut job = super::tests::create(&mut store, "team");
    let secret = "supplied-secret";
    job.id = format!("CODETERM_API_TOKEN={secret}");
    let bucket = "supplied_bucket";
    job.buckets = vec![bucket.into()];
    let scope = Scope::from(&job);
    let outcome = machine::evaluate(
        &mut job.machine,
        b"opaque-output",
        &job.buckets,
        &job.routes,
        None,
        scope,
        || {
            Classification::adapter(Ok(machine::Verdict {
                bucket: bucket.into(),
                confidence: 0.9,
            }))
        },
    );
    scheduler::record(&runtime, &job, &outcome, Stage::Decision).unwrap();
    let feed = runtime.feed(10, None).unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0]["asked"][0]["answer"], bucket.replace('_', " "));
    assert!(!feed[0].to_string().contains(secret));
    assert!(feed[0].get("probe").is_none());
    assert!(feed[0].get("probe_output").is_none());
}

pub(super) fn assert_budget_skip_is_journaled() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = Arc::new(SupporterRuntime::new(dir.path().join("supporter")));
    // 0 means no cap now; an explicit cap of 1 already spent forces exhaustion.
    runtime
        .reserve(1, VerdictKind::Sentinel, Some("creator"), 1)
        .expect("first request fits the explicit cap");
    let model = supporter::budgeted_model_with_activity(
        Arc::new(Model {
            calls: calls.clone(),
        }),
        runtime.clone(),
        1,
        VerdictKind::Sentinel,
        Some("creator".into()),
        None,
        None,
    );
    let mut store = super::tests::store(
        dir.path(),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let mut job = super::tests::create(&mut store, "team");
    let request = classify::request(&job, "private");
    let scope = Scope::from(&job);
    let outcome = machine::evaluate(
        &mut job.machine,
        b"private",
        &job.buckets,
        &job.routes,
        None,
        scope,
        || {
            classify::decide(
                model.clone(),
                &request,
                Instant::now() + Duration::from_secs(1),
            )
        },
    );
    assert_eq!(outcome.reason, "budget_exhausted");
    assert!(outcome.route.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    scheduler::record(&runtime, &job, &outcome, Stage::Skip).unwrap();
    let journal = std::fs::read_to_string(dir.path().join("supporter/journal.jsonl")).unwrap();
    let entries: Vec<serde_json::Value> = journal
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        entries
            .iter()
            .filter(|v| v["reason"] == "budget_exhausted" && v["action"] == "skip")
            .count(),
        1
    );
    let skipped = entries
        .iter()
        .find(|entry| entry["reason"] == "budget_exhausted" && entry["action"] == "skip")
        .unwrap();
    assert_eq!(skipped["classification_source"], "adapter");
    assert_eq!(skipped["classification_reason"], "budget_exhausted");
    assert!(skipped["observation_id"].is_string());
    assert!(skipped["label"].is_null());
    assert_eq!(skipped["route_status"], "not_attempted");
    assert!(skipped["route"].is_null());
    assert!(!journal.contains("private"));
}
