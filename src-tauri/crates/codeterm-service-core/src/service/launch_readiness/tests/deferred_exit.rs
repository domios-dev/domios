use super::*;
use crate::service::process_tree::{
    ProcessIdentity, ProviderProcessTreeSnapshot, ProviderTreeWitnesses,
};

fn attempt(prefix: &str) -> LaunchAttempt {
    LaunchAttempt {
        tab_id: format!("{prefix}-tab"),
        pty_id: format!("{prefix}-pty"),
        binding_nonce: format!("{prefix}-nonce"),
    }
}

fn clean_exit_before_provider_seen(attempt: &LaunchAttempt) -> LaunchExitProof {
    let witnesses = ProviderTreeWitnesses::default();
    let key = provider_tree_key(attempt, "fixture-agent");
    witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let absent = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    LaunchExitProof::from_finished_marker(attempt, &absent, Some(0), false, Some(false))
        .expect("two absences and no named process prove a clean exit")
}

fn expired() -> Instant {
    Instant::now()
}

#[test]
fn clean_exit_before_sighting_is_deferred_and_a_later_binding_wins() {
    let attempt = attempt("deferred-then-bound");
    let proof = clean_exit_before_provider_seen(&attempt);
    assert!(proof.is_deferred());

    arm(attempt.clone());
    assert!(suspect_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        Some(0),
        &proof
    ));
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert_eq!(
        observe_with_owner_prompt(&attempt, expired(), |_| {}),
        LaunchObservation::Evidence(LaunchEvidence::Acknowledged)
    );
}

#[test]
fn clean_exit_before_sighting_without_binding_fails_at_the_deadline() {
    let attempt = attempt("deferred-no-binding");
    let proof = clean_exit_before_provider_seen(&attempt);

    arm(attempt.clone());
    assert!(suspect_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        Some(0),
        &proof
    ));
    let LaunchObservation::SuspectedExit {
        exit_code,
        proof: carried,
    } = observe_with_owner_prompt(&attempt, expired(), |_| {})
    else {
        panic!("a deferred exit with no binding surfaces at the deadline");
    };
    assert_eq!(exit_code, Some(0));
    assert_eq!(carried, proof);
    assert!(claim_exit(&attempt.tab_id, Some(&attempt.pty_id), exit_code, &carried).is_some());
}

#[test]
fn acknowledged_launch_refuses_a_late_suspicion() {
    let attempt = attempt("bound-then-suspected");
    let proof = clean_exit_before_provider_seen(&attempt);

    arm(attempt.clone());
    assert!(note_acknowledged(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce
    ));
    assert!(!suspect_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        Some(0),
        &proof
    ));
    retire_attempt(&attempt);
}

#[test]
fn unarmed_launch_settles_a_clean_exit_at_once() {
    let attempt = attempt("unarmed");
    let proof = clean_exit_before_provider_seen(&attempt);
    assert!(!suspect_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        Some(0),
        &proof
    ));
}

#[test]
fn failed_or_witnessed_exits_are_never_deferred() {
    let attempt = attempt("strong-exit");
    let witnesses = ProviderTreeWitnesses::default();
    let key = provider_tree_key(&attempt, "fixture-agent");
    let absent = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let failed = LaunchExitProof::from_finished_marker(&attempt, &absent, Some(1), true, None)
        .expect("a non-zero exit is explicit evidence");
    assert!(!failed.is_deferred());

    witnesses.record_for_test(
        &key,
        ProviderProcessTreeSnapshot::Present(
            [ProcessIdentity {
                pid: 61_001,
                start_token: "fixture-agent-start".into(),
            }]
            .into_iter()
            .collect(),
        ),
    );
    let gone = witnesses.record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let witnessed = LaunchExitProof::from_finished_marker(&attempt, &gone, Some(0), false, None)
        .expect("a provider seen then gone proves the exit");
    assert!(!witnessed.is_deferred());
}
