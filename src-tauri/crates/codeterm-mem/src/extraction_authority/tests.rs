use super::*;
use parking_lot::Mutex;

#[derive(Default)]
struct RecordingSink {
    calls: Mutex<Vec<Vec<RevokedGeneration>>>,
}

impl ExtractionCancelSink for RecordingSink {
    fn cancel(&self, revoked: &[RevokedGeneration]) {
        self.calls.lock().push(revoked.to_vec());
    }
}

#[test]
fn revoking_a_source_refuses_its_generations_commits() {
    let auth = ExtractionAuthority::new();
    let generation = auth.open("note_a", "uid_a");
    auth.bind_worker(&generation, "gw_1");

    assert_eq!(
        auth.authorize_worker(Some("gw_1")),
        CommitAuthority::Authorized(generation.clone())
    );

    auth.revoke_source("note_a");

    let after = auth.authorize_worker(Some("gw_1"));
    assert!(after.is_revoked(), "revoked generation must lose authority");
    assert!(after.refusal().unwrap().contains("note_a"));
}

/// The late case: the Worker's model execution already began (it is bound and
/// authorized) when the delete lands. Authority is checked at commit time, so
/// the commit is still refused. No timing is involved.
#[test]
fn generation_already_executing_is_refused_at_commit_time() {
    let auth = ExtractionAuthority::new();
    let generation = auth.open("note_late", "uid_late");
    assert_eq!(
        auth.bind_worker(&generation, "gw_late"),
        BindOutcome::Active
    );
    assert!(matches!(
        auth.authorize_worker(Some("gw_late")),
        CommitAuthority::Authorized(_)
    ));

    auth.revoke_source("note_late");

    assert!(auth.authorize_worker(Some("gw_late")).is_revoked());
    assert!(auth.is_revoked(&generation));
}

/// Delete-then-recreate reuses the slug id. The old generation must stay
/// refused even though a newer live generation now exists for the same id —
/// this is the latest-or-global fallback the repo has shipped before.
#[test]
fn recreated_source_does_not_reauthorize_the_old_generation() {
    let auth = ExtractionAuthority::new();
    let old = auth.open("note_x", "uid_v1");
    auth.bind_worker(&old, "gw_old");
    auth.revoke_source("note_x");

    let new = auth.open("note_x", "uid_v2");
    auth.bind_worker(&new, "gw_new");

    assert_ne!(old, new, "recreate must allocate a distinct generation");
    assert!(
        auth.authorize_worker(Some("gw_old")).is_revoked(),
        "old generation must not be revived by a newer generation on the same id"
    );
    assert_eq!(
        auth.authorize_worker(Some("gw_new")),
        CommitAuthority::Authorized(new)
    );
}

#[test]
fn revocation_is_scoped_to_the_exact_source() {
    let auth = ExtractionAuthority::new();
    let doomed = auth.open("note_doomed", "uid_1");
    let sibling = auth.open("note_sibling", "uid_2");
    auth.bind_worker(&doomed, "gw_doomed");
    auth.bind_worker(&sibling, "gw_sibling");

    let revoked = auth.revoke_source("note_doomed");

    assert_eq!(revoked.len(), 1, "only the deleted source is revoked");
    assert_eq!(revoked[0].worker_id.as_deref(), Some("gw_doomed"));
    assert_eq!(
        auth.authorize_worker(Some("gw_sibling")),
        CommitAuthority::Authorized(sibling),
        "an unrelated extraction generation keeps its authority"
    );
}

#[test]
fn unknown_worker_is_unattributed_not_guessed() {
    let auth = ExtractionAuthority::new();
    let generation = auth.open("note_a", "uid_a");
    auth.bind_worker(&generation, "gw_known");
    auth.revoke_source("note_a");

    assert_eq!(
        auth.authorize_worker(Some("gw_stranger")),
        CommitAuthority::Unattributed,
        "an unknown worker must not inherit any source's generation"
    );
    assert_eq!(auth.authorize_worker(None), CommitAuthority::Unattributed);
}

/// Delete landing between `open` and `bind_worker` is the spawn race. The bind
/// reports it synchronously so the spawn site cancels without polling.
#[test]
fn bind_after_revoke_reports_already_revoked() {
    let auth = ExtractionAuthority::new();
    let generation = auth.open("note_race", "uid_race");

    let revoked = auth.revoke_source("note_race");
    assert_eq!(revoked.len(), 1);
    assert_eq!(
        revoked[0].worker_id, None,
        "no worker bound yet, so none can be named"
    );

    assert_eq!(
        auth.bind_worker(&generation, "gw_race"),
        BindOutcome::AlreadyRevoked
    );
    assert!(auth.authorize_worker(Some("gw_race")).is_revoked());
}

#[test]
fn cancel_sink_receives_exactly_the_revoked_generations() {
    let auth = ExtractionAuthority::new();
    let sink = Arc::new(RecordingSink::default());
    auth.set_cancel_sink(sink.clone());

    let doomed = auth.open("note_doomed", "uid_1");
    auth.bind_worker(&doomed, "gw_doomed");
    let sibling = auth.open("note_sibling", "uid_2");
    auth.bind_worker(&sibling, "gw_sibling");

    auth.revoke_source("note_doomed");

    let calls = sink.calls.lock();
    assert_eq!(calls.len(), 1, "one revocation, one cancellation batch");
    assert_eq!(calls[0].len(), 1);
    assert_eq!(calls[0][0].worker_id.as_deref(), Some("gw_doomed"));
}

#[test]
fn revoking_a_source_with_no_generations_does_not_notify() {
    let auth = ExtractionAuthority::new();
    let sink = Arc::new(RecordingSink::default());
    auth.set_cancel_sink(sink.clone());

    let revoked = auth.revoke_source("note_never_extracted");

    assert!(revoked.is_empty());
    assert!(
        sink.calls.lock().is_empty(),
        "no generation means nothing to cancel"
    );
}

#[test]
fn closed_generation_is_no_longer_open_but_revoked_stays_refused() {
    let auth = ExtractionAuthority::new();
    let finished = auth.open("note_done", "uid_done");
    auth.bind_worker(&finished, "gw_done");
    auth.close(&finished);

    assert!(auth.open_generations("note_done").is_empty());
    assert_eq!(
        auth.revoke_source("note_done").len(),
        0,
        "a generation that already finished has nothing to revoke"
    );

    let revoked = auth.open("note_r", "uid_r");
    auth.bind_worker(&revoked, "gw_r");
    auth.revoke_source("note_r");
    auth.close(&revoked);
    assert!(
        auth.authorize_worker(Some("gw_r")).is_revoked(),
        "closing must not clear a revocation"
    );
}

#[test]
fn concurrent_generations_on_one_source_are_all_revoked() {
    let auth = ExtractionAuthority::new();
    let first = auth.open("note_multi", "uid_1");
    let second = auth.open("note_multi", "uid_2");
    auth.bind_worker(&first, "gw_1");
    auth.bind_worker(&second, "gw_2");

    let revoked = auth.revoke_source("note_multi");

    assert_eq!(revoked.len(), 2);
    assert!(auth.authorize_worker(Some("gw_1")).is_revoked());
    assert!(auth.authorize_worker(Some("gw_2")).is_revoked());
}
