use super::machine::*;
use std::collections::BTreeMap;
use std::cell::Cell;

fn buckets() -> Vec<String> { vec!["waiting".into(), "passed".into(), "failed".into()] }
fn verdict(bucket: &str, confidence: f64) -> Result<Verdict, &'static str> {
    Ok(Verdict { bucket: bucket.into(), confidence })
}
fn tick(machine: &mut Machine, output: &[u8], bucket: &str, confidence: f64) -> Outcome {
    evaluate(machine, output, &buckets(), &BTreeMap::new(), None, || verdict(bucket, confidence))
}

#[test]
fn sentinel_unchanged_output_makes_no_call() {
    let mut machine = Machine::default();
    let calls = Cell::new(0);
    for _ in 0..3 {
        evaluate(&mut machine, b"fixed", &buckets(), &BTreeMap::new(), None, || {
            calls.set(calls.get() + 1); verdict("waiting", 0.9)
        });
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn sentinel_transition_routes_once() {
    let mut machine = Machine::default();
    assert_eq!(tick(&mut machine, b"1", "waiting", 0.80).route, Some(Route::Notify));
    assert_eq!(tick(&mut machine, b"2", "waiting", 0.99).route, None);
    assert_eq!(tick(&mut machine, b"3", "passed", 0.9).route, Some(Route::Notify));
    assert_eq!(tick(&mut machine, b"4", "passed", 0.9).route, None);
    assert_eq!(tick(&mut machine, b"5", "waiting", 0.9).route, Some(Route::Notify));
}

#[test]
fn sentinel_other_and_low_confidence_no_route() {
    let mut machine = Machine::default();
    for (output, bucket, confidence) in [(b"1", "other", 1.0), (b"2", "passed", 0.79), (b"3", "unknown", 0.99), (b"4", "passed", f64::NAN)] {
        assert_eq!(tick(&mut machine, output, bucket, confidence).route, None);
    }
    assert_eq!(machine.last_bucket, None);
    assert_eq!(tick(&mut machine, b"5", "passed", 0.9).route, Some(Route::Notify));
}

#[test]
fn sentinel_budget_exhausted_no_route_journaled() {
    let mut machine = Machine::default();
    let out = evaluate(&mut machine, b"1", &buckets(), &BTreeMap::new(), None, || Err("budget_exhausted"));
    assert_eq!(out.reason, "budget_exhausted");
    assert_eq!(out.route, None);
    let out = evaluate(&mut machine, b"1", &buckets(), &BTreeMap::new(), None, || panic!("unchanged output must not retry the budget"));
    assert_eq!(out.reason, "unchanged");
}

#[test]
fn sentinel_until_is_one_shot() {
    let mut machine = Machine::default();
    let routes = BTreeMap::from([("passed".into(), Route::Send("tab".into()))]);
    let out = evaluate(&mut machine, b"1", &buckets(), &routes, Some("passed"), || verdict("passed", 0.9));
    assert_eq!(out.route, Some(Route::Send("tab".into())));
    assert!(machine.stopped);
    let out = evaluate(&mut machine, b"2", &buckets(), &routes, None, || panic!("stopped sentinel must not classify"));
    assert_eq!(out.route, None);
}

#[test]
fn sentinel_regex_fallback_without_adapter() {
    let patterns = BTreeMap::from([("passed".into(), "^success$".into()), ("failed".into(), "^failure$".into())]);
    validate(&buckets(), &patterns, false, MIN_INTERVAL_MS).unwrap();
    let v = regex_verdict(&patterns, "success");
    assert_eq!(v, Verdict { bucket: "passed".into(), confidence: 1.0 });
    let mut machine = Machine::default();
    assert_eq!(tick(&mut machine, b"success", &v.bucket, v.confidence).route, Some(Route::Notify));
    assert_eq!(regex_verdict(&patterns, "unrecognized").bucket, "other");
    let overlapping = BTreeMap::from([("passed".into(), "success".into()), ("failed".into(), "success".into())]);
    assert_eq!(regex_verdict(&overlapping, "success").bucket, "other");
}

#[test]
fn sentinel_create_refused_without_adapter_or_pattern() {
    assert!(validate(&buckets(), &BTreeMap::new(), false, MIN_INTERVAL_MS).unwrap_err().contains("--match"));
    assert!(validate(&buckets(), &BTreeMap::new(), true, MIN_INTERVAL_MS).is_ok());
}

#[test]
fn sentinel_min_interval_enforced() {
    assert!(validate(&buckets(), &BTreeMap::new(), true, MIN_INTERVAL_MS - 1).is_err());
    assert!(validate(&buckets(), &BTreeMap::new(), true, MIN_INTERVAL_MS).is_ok());
}

#[test]
fn sentinel_routes_are_closed_set() {
    for value in ["notify", "send:tab", "cron:name", "stop"] { assert!(Route::parse(value).is_ok()); }
    for value in ["sh:command", "send:", "cron:", "command", "notify;evil"] { assert!(Route::parse(value).is_err()); }
}
