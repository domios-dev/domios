use std::path::Path;

#[test]
fn only_default_production_instance_updates_without_opt_in() {
    assert_eq!(
        super::update_instance_name(None, 7685, false).as_deref(),
        Some("production")
    );
    assert_eq!(super::update_instance_name(None, 7686, false), None);
    assert_eq!(
        super::update_instance_name(Some("dev-a"), 7685, false),
        None
    );
    assert_eq!(
        super::update_instance_name(Some("dev-a"), 7686, true).as_deref(),
        Some("dev-a")
    );
}

#[test]
fn npm_windows_shim_resolves_to_a_direct_node_script() {
    let parent = Path::new("C:/Users/owner/AppData/Roaming/npm");
    let shim = "@ECHO off\n\"%_prog%\" \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\n";
    assert_eq!(
        super::command::npm_shim_script(parent, shim),
        Some(parent.join("node_modules/@openai/codex/bin/codex.js"))
    );
}

use super::state::{Admission, Coordinator, RunOutcome};

#[test]
fn update_coordinator_dedupes_provider_and_target_across_reload() {
    let mut state = Coordinator::default();
    assert_eq!(state.begin("fixture", "1", "2"), Admission::Started);
    assert_eq!(state.begin("fixture", "1", "2"), Admission::Duplicate);
    assert_eq!(state.begin("fixture", "1", "3"), Admission::ProviderBusy);
    let mut restored: Coordinator =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(restored.begin("fixture", "1", "2"), Admission::Duplicate);
    restored.finish("fixture:2", Ok(()));
    assert_eq!(restored.runs["fixture:2"].outcome, RunOutcome::Succeeded);
    assert_eq!(restored.begin("fixture", "2", "3"), Admission::Started);
}

#[test]
fn update_coordinator_provider_scopes_are_independent() {
    let mut state = Coordinator::default();
    assert_eq!(state.begin("first", "1", "2"), Admission::Started);
    assert_eq!(state.begin("second", "1", "2"), Admission::Started);
    state.finish("first:2", Err("opaque failure".into()));
    assert_eq!(state.begin("first", "1", "2"), Admission::Duplicate);
    assert_eq!(state.begin("first", "1", "3"), Admission::Started);
}

#[test]
fn update_coordinator_unstopped_and_interrupted_updaters_block_further_runs() {
    let mut state = Coordinator::default();
    state.begin("fixture", "1", "2");
    state.finish("fixture:2", Err("opaque failure".into()));
    state.block_unstopped("fixture:2");
    assert_eq!(state.begin("fixture", "1", "3"), Admission::ProviderBusy);
    state.begin("other", "1", "2");
    state.recover_interrupted(|_| false);
    assert_eq!(state.begin("other", "1", "2"), Admission::Duplicate);
    assert_eq!(state.begin("other", "1", "3"), Admission::ProviderBusy);
}

#[test]
fn update_coordinator_machine_claim_dedupes_concurrent_tabs_and_disk_reload() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let threads = (0..4)
        .map(|_| {
            let barrier = barrier.clone();
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                barrier.wait();
                super::storage::transact_in(&path, |state| state.begin("fixture", "1", "2"))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    let admissions = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        admissions
            .iter()
            .filter(|admission| **admission == Admission::Started)
            .count(),
        1
    );
    assert_eq!(
        admissions
            .iter()
            .filter(|admission| **admission == Admission::Duplicate)
            .count(),
        3
    );
    super::storage::transact_in(dir.path(), |state| {
        assert_eq!(state.begin("fixture", "1", "2"), Admission::Duplicate);
        state.finish("fixture:2", Ok(()));
    })
    .unwrap();
    super::storage::transact_in(dir.path(), |state| {
        assert_eq!(state.begin("fixture", "1", "2"), Admission::Duplicate);
        assert_eq!(state.runs["fixture:2"].outcome, RunOutcome::Succeeded);
    })
    .unwrap();
}

#[test]
fn update_coordinator_journal_correlates_provider_target_and_tab() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = codeterm_supporter::SupporterRuntime::new(dir.path().into());
    runtime
        .provider_update_action(
            "fixture",
            Some("2"),
            codeterm_supporter::ActionRecord {
                kind: codeterm_supporter::VerdictKind::TabState,
                tab: Some("tab-a"),
                label: "resumed",
                confidence: 1.0,
                action: codeterm_supporter::Action::Recovery,
                reason: "provider_update_restart",
                asked: None,
            },
        )
        .unwrap();
    let contents = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let entry: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
    assert_eq!(entry["provider"], "fixture");
    assert_eq!(entry["target_version"], "2");
    assert_eq!(entry["tab"], "tab-a");
    assert_eq!(entry["label"], "resumed");
}

#[test]
fn update_coordinator_corrupt_durable_claim_refuses_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(super::storage::STATE_FILE), b"invalid-json").unwrap();
    let called = std::sync::atomic::AtomicBool::new(false);
    let result = super::storage::transact_in(dir.path(), |_| {
        called.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    assert!(result.is_err());
    assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn startup_check_waits_for_restore_completion_and_runs_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let gate = super::boot::RestoreGate::new();
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    gate.defer(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(
        runs.load(Ordering::SeqCst),
        0,
        "boot must not run the check"
    );
    gate.open();
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    gate.open();
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "later settles never rerun it"
    );
}

#[test]
fn startup_check_registered_after_restore_runs_immediately() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let gate = super::boot::RestoreGate::new();
    gate.open();
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    gate.defer(Box::new(move || flag.store(true, Ordering::SeqCst)));
    assert!(ran.load(Ordering::SeqCst));
}

#[test]
fn availability_check_is_skipped_within_a_day_per_provider() {
    let day = super::state::CHECK_INTERVAL_MS;
    let mut state = Coordinator::default();
    assert!(state.claim_check("first", 1_000));
    assert!(!state.claim_check("first", 1_000 + day - 1));
    assert!(state.claim_check("second", 1_000 + day - 1));
    let mut restored: Coordinator =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert!(!restored.claim_check("first", 1_000 + day - 1));
    assert!(restored.claim_check("first", 1_000 + day));
}

#[test]
fn state_without_check_stamps_still_loads() {
    let legacy: Coordinator = serde_json::from_str(r#"{"runs":{}}"#).unwrap();
    assert!(legacy.checked.is_empty());
}

#[test]
fn check_stamp_persists_through_the_state_file() {
    let dir = tempfile::tempdir().unwrap();
    let day = super::state::CHECK_INTERVAL_MS;
    assert!(
        super::storage::transact_in(dir.path(), |state| state.claim_check("fixture", 5)).unwrap()
    );
    assert!(!super::storage::transact_in(dir.path(), |state| state
        .claim_check("fixture", 5 + day / 2))
    .unwrap());
}
