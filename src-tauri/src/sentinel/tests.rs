use super::machine::*;
use super::provenance::{Classification, Failure, Scope};
use std::cell::Cell;
use std::collections::BTreeMap;

fn buckets() -> Vec<String> {
    vec!["waiting".into(), "passed".into(), "failed".into()]
}
fn verdict(bucket: &str, confidence: f64) -> Classification {
    Classification::adapter(Ok(Verdict {
        bucket: bucket.into(),
        confidence,
    }))
}
fn scope() -> Scope {
    Scope {
        sentinel_id: "watch".into(),
        creator_tab_id: "creator".into(),
        team_id: Some("team".into()),
        workspace_id: "workspace".into(),
    }
}
fn tick(machine: &mut Machine, output: &[u8], bucket: &str, confidence: f64) -> Outcome {
    evaluate(
        machine,
        output,
        &buckets(),
        &BTreeMap::new(),
        None,
        scope(),
        || verdict(bucket, confidence),
    )
}

#[test]
fn sentinel_unchanged_output_makes_no_call() {
    let mut machine = Machine::default();
    let calls = Cell::new(0);
    for _ in 0..3 {
        evaluate(
            &mut machine,
            b"fixed",
            &buckets(),
            &BTreeMap::new(),
            None,
            scope(),
            || {
                calls.set(calls.get() + 1);
                verdict("waiting", 0.9)
            },
        );
    }
    assert_eq!(calls.get(), 1);
}

#[test]
fn sentinel_transition_routes_once() {
    let mut machine = Machine::default();
    assert_eq!(
        tick(&mut machine, b"1", "waiting", 0.80).route,
        Some(Route::Notify)
    );
    assert_eq!(tick(&mut machine, b"2", "waiting", 0.99).route, None);
    assert_eq!(
        tick(&mut machine, b"3", "passed", 0.9).route,
        Some(Route::Notify)
    );
    assert_eq!(tick(&mut machine, b"4", "passed", 0.9).route, None);
    assert_eq!(
        tick(&mut machine, b"5", "waiting", 0.9).route,
        Some(Route::Notify)
    );
}

#[test]
fn sentinel_other_and_low_confidence_no_route() {
    let mut machine = Machine::default();
    for (output, bucket, confidence) in [
        (b"1", "other", 1.0),
        (b"2", "passed", 0.79),
        (b"3", "unknown", 0.99),
        (b"4", "passed", f64::NAN),
    ] {
        assert_eq!(tick(&mut machine, output, bucket, confidence).route, None);
    }
    assert_eq!(machine.last_bucket, None);
    assert_eq!(
        tick(&mut machine, b"5", "passed", 0.9).route,
        Some(Route::Notify)
    );
}

#[test]
fn sentinel_budget_exhausted_no_route_journaled() {
    super::connected_tests::assert_budget_skip_is_journaled();
    let mut machine = Machine::default();
    let out = evaluate(
        &mut machine,
        b"1",
        &buckets(),
        &BTreeMap::new(),
        None,
        scope(),
        || Classification::adapter(Err(Failure::Budget)),
    );
    assert_eq!(out.reason, "budget_exhausted");
    assert_eq!(out.route, None);
    let out = evaluate(
        &mut machine,
        b"1",
        &buckets(),
        &BTreeMap::new(),
        None,
        scope(),
        || panic!("unchanged output must not retry the budget"),
    );
    assert_eq!(out.reason, "unchanged");
}

#[test]
fn sentinel_until_is_one_shot() {
    let mut machine = Machine::default();
    let routes = BTreeMap::from([("passed".into(), Route::Send("tab".into()))]);
    let out = evaluate(
        &mut machine,
        b"1",
        &buckets(),
        &routes,
        Some("passed"),
        scope(),
        || verdict("passed", 0.9),
    );
    assert_eq!(out.route, Some(Route::Send("tab".into())));
    assert!(machine.stopped);
    let out = evaluate(
        &mut machine,
        b"2",
        &buckets(),
        &routes,
        None,
        scope(),
        || panic!("stopped sentinel must not classify"),
    );
    assert_eq!(out.route, None);
}

#[test]
fn sentinel_regex_fallback_without_adapter() {
    let patterns = BTreeMap::from([
        ("passed".into(), "^success$".into()),
        ("failed".into(), "^failure$".into()),
    ]);
    validate(&buckets(), &patterns, false, MIN_INTERVAL_MS).unwrap();
    let v = regex_verdict(&patterns, "success");
    assert_eq!(
        v,
        Verdict {
            bucket: "passed".into(),
            confidence: 1.0
        }
    );
    let mut machine = Machine::default();
    assert_eq!(
        tick(&mut machine, b"success", &v.bucket, v.confidence).route,
        Some(Route::Notify)
    );
    assert_eq!(regex_verdict(&patterns, "unrecognized").bucket, "other");
    let overlapping = BTreeMap::from([
        ("passed".into(), "success".into()),
        ("failed".into(), "success".into()),
    ]);
    assert_eq!(regex_verdict(&overlapping, "success").bucket, "other");
}

#[test]
fn sentinel_create_refused_without_adapter_or_pattern() {
    assert!(
        validate(&buckets(), &BTreeMap::new(), false, MIN_INTERVAL_MS)
            .unwrap_err()
            .contains("--match")
    );
    assert!(validate(&buckets(), &BTreeMap::new(), true, MIN_INTERVAL_MS).is_ok());
}

#[test]
fn sentinel_min_interval_enforced() {
    assert!(validate(&buckets(), &BTreeMap::new(), true, MIN_INTERVAL_MS - 1).is_err());
    assert!(validate(&buckets(), &BTreeMap::new(), true, MIN_INTERVAL_MS).is_ok());
}

#[test]
fn sentinel_routes_are_closed_set() {
    for value in ["notify", "send:tab", "cron:name", "stop"] {
        assert!(Route::parse(value).is_ok());
    }
    for value in ["sh:command", "send:", "cron:", "command", "notify;evil"] {
        assert!(Route::parse(value).is_err());
    }
}

fn create_params() -> super::store::Create {
    super::store::Create {
        probe: "check-ci".into(),
        question: "CI result?".into(),
        buckets: buckets(),
        every: "10s".into(),
        routes: BTreeMap::new(),
        matches: BTreeMap::new(),
        until: None,
        timeout: None,
    }
}

pub(super) fn store(
    dir: &std::path::Path,
    clock: crate::cron::clock::SharedClock,
) -> super::store::Store {
    super::store::Store::new(dir.join("sentinels.json"), clock)
}

pub(super) fn create(store: &mut super::store::Store, team: &str) -> super::store::Sentinel {
    store
        .create(
            create_params(),
            crate::sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: Some(team.into()),
                workspace_id: "workspace".into(),
                cwd: "/tmp".into(),
            },
            true,
        )
        .unwrap()
}

#[test]
fn sentinel_removed_on_team_close() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = store(dir.path(), clock.clone());
    let removed = create(&mut store, "team-a");
    let retained = create(&mut store, "team-b");
    let leases = store.due().unwrap();
    let cancelled = leases
        .iter()
        .find(|lease| lease.job.id == removed.id)
        .unwrap()
        .cancelled
        .clone();
    store.remove_team("team-a").unwrap();
    assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
    assert!(store.get(&removed.id).is_none());
    assert!(store.get(&retained.id).is_some());
    assert!(!store.finish(&removed).unwrap());
    let reloaded = super::store::Store::new(dir.path().join("sentinels.json"), clock);
    assert!(reloaded.get(&removed.id).is_none());
    assert!(reloaded.get(&retained.id).is_some());
}

#[test]
fn sentinel_timeout_and_cadence_use_cron_clock() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = store(dir.path(), clock.clone());
    let mut params = create_params();
    params.timeout = Some("20s".into());
    let job = store
        .create(
            params,
            crate::sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "ws".into(),
                cwd: "/tmp".into(),
            },
            true,
        )
        .unwrap();
    let first = store.due().unwrap().pop().unwrap();
    assert_eq!(first.job.next_run, 11_000);
    assert!(store.due().unwrap().is_empty());
    store.release(&job.id);
    clock.advance_ms(10_000);
    assert_eq!(store.due().unwrap().len(), 1);
    clock.advance_ms(10_000);
    assert!(store.due().unwrap().is_empty());
    assert!(store.get(&job.id).unwrap().machine.stopped);
    assert_eq!(
        store.get(&job.id).unwrap().last_reason.as_deref(),
        Some("timeout")
    );
    assert!(!store.finish(&first.job).unwrap());
}

#[test]
fn sentinel_stop_during_model_call_cancels_route_claim() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(dir.path(), crate::cron::clock::MockClock::starting_at(1000));
    let job = create(&mut store, "team");
    let mut lease = store.due().unwrap().pop().unwrap();
    store.stop(&job.id, "manual").unwrap();
    let result = tick(&mut lease.job.machine, b"done", "passed", 1.0);
    assert!(result.route.is_some());
    assert!(lease.cancelled.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!store.finish(&lease.job).unwrap());
}

#[test]
fn sentinel_duration_overflow_refused() {
    for value in [
        "18446744073709551615d",
        "0s",
        "99999999999999999999999999999999999s",
        "in 10s",
        "1s2",
        "",
    ] {
        assert!(super::store::duration_ms(value).is_err(), "{value}");
    }
    assert_eq!(super::store::duration_ms("1h30m").unwrap(), 5_400_000);
}

#[test]
fn sentinel_probe_bounded_runtime_and_output() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let (large, stalled) = (
        "yes x | head -c 1000000",
        "read x < /dev/zero; exec cat /dev/zero",
    );
    #[cfg(windows)]
    let (large, stalled) = (
        "[Console]::Out.Write(('x' * 200000))",
        "while ($true) { [Console]::Out.Write('x') }",
    );
    // A fresh CI runner's first powershell.exe start can exceed any probe limit; warm it first.
    #[cfg(windows)]
    {
        let mut warm = std::process::Command::new("powershell.exe");
        crate::process_util::no_window_std(&mut warm);
        let _ = warm
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "exit",
            ])
            .status();
    }
    let started = std::time::Instant::now();
    let trivial = super::probe::run("echo ok", dir.path(), super::probe::PROBE_RUNTIME);
    assert!(
        trivial.is_ok(),
        "trivial probe failed after {:?}: {:?}",
        started.elapsed(),
        trivial.err()
    );
    let started = std::time::Instant::now();
    let output =
        super::probe::run(large, dir.path(), super::probe::PROBE_RUNTIME).unwrap_or_else(|error| {
            panic!("large probe failed after {:?}: {error}", started.elapsed())
        });
    assert!(output.bytes.len() <= super::probe::MAX_OUTPUT_BYTES + 12);
    assert!(output.bytes.len() >= super::probe::MAX_OUTPUT_BYTES / 2);
    let began = std::time::Instant::now();
    let result = super::probe::run(stalled, dir.path(), std::time::Duration::from_millis(200));
    assert_eq!(result.unwrap_err(), "probe_timeout");
    assert!(began.elapsed() < std::time::Duration::from_secs(3));
}

#[test]
fn sentinel_transition_and_hash_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let clock = crate::cron::clock::MockClock::starting_at(1000);
    let mut store = store(dir.path(), clock.clone());
    let created = create(&mut store, "team");
    let mut job = store.due().unwrap().pop().unwrap().job;
    assert_eq!(
        tick(&mut job.machine, b"1", "passed", 0.9).route,
        Some(Route::Notify)
    );
    assert!(store.finish(&job).unwrap());
    drop(store);
    let store = super::store::Store::new(dir.path().join("sentinels.json"), clock);
    let mut restored = store.get(&created.id).unwrap().clone();
    let skipped = evaluate(
        &mut restored.machine,
        b"1",
        &restored.buckets,
        &restored.routes,
        None,
        scope(),
        || panic!("restored unchanged hash must skip"),
    );
    assert_eq!(skipped.reason, "unchanged");
    assert_eq!(tick(&mut restored.machine, b"2", "passed", 0.9).route, None);
}

#[test]
fn sentinel_create_patterns_and_routes_are_validated_before_admission() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = store(dir.path(), crate::cron::clock::MockClock::starting_at(1000));
    let mut params = create_params();
    assert!(store
        .create(
            params.clone(),
            crate::sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "ws".into(),
                cwd: "/tmp".into()
            },
            false
        )
        .unwrap_err()
        .contains("--match"));
    params.matches.insert("passed".into(), "(".into());
    assert!(store
        .create(
            params.clone(),
            crate::sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "ws".into(),
                cwd: "/tmp".into()
            },
            false
        )
        .is_err());
    params.matches.insert("passed".into(), "success".into());
    params.routes.insert("passed".into(), "command:evil".into());
    assert!(store
        .create(
            params.clone(),
            crate::sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "ws".into(),
                cwd: "/tmp".into()
            },
            false
        )
        .is_err());
    assert!(store.list().is_empty());
    params.routes.insert("passed".into(), "stop".into());
    let job = store
        .create(
            params,
            crate::sentinel::store::Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "ws".into(),
                cwd: "/tmp".into(),
            },
            false,
        )
        .unwrap();
    assert_eq!(job.routes["passed"], Route::Stop);
}
