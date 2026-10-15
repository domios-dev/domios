use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use codeterm_cron::clock::MockClock;
use crate::store::{Create, Owner};
use parking_lot::Mutex;

fn fixture() -> (tempfile::TempDir, Store, Sentinel) {
    let root = tempfile::tempdir().unwrap();
    let mut store = Store::new(
        root.path().join("sentinels.json"),
        MockClock::starting_at(1000),
    );
    let job = store
        .create(
            Create {
                terminal: None,
                session: false,
                name: Some("initial".into()),
                probe: Some("status".into()),
                observe: None,
                input: None,
                question: "Is it done?".into(),
                buckets: vec!["done".into()],
                every: "10s".into(),
                routes: BTreeMap::new(),
                model_classification: false,
                matches: BTreeMap::from([("done".into(), "done".into())]),
                rearm_on_change: Default::default(),
                until: None,
                timeout: None,
                paused: false,
                handlers: None,
                escalation_timeout_ms: None,
                max_transitions: None,
                max_actions: None,
            },
            Owner {
                creator_tab_id: "creator".into(),
                team_id: Some("team".into()),
                workspace_id: "workspace".into(),
                cwd: "/tmp".into(),
            },
            true,
        )
        .unwrap();
    (root, store, job)
}

fn update(question: &str) -> Update {
    Update {
        name: Some("updated".into()),
        question: question.into(),
        buckets: vec!["done".into()],
        every: Some("30s".into()),
        routes: BTreeMap::from([("done".into(), "notify".into())]),
        model_classification: false,
        matches: BTreeMap::from([("done".into(), "done".into())]),
        rearm_on_change: Default::default(),
        until: None,
        input: None,
        handlers: None,
        on_other: Vec::new(),
        escalation_timeout_ms: None,
        max_transitions: Some(64),
        max_actions: Some(256),
        cause: Some("transition_4".into()),
        note: Some("tightened completion rule".into()),
    }
}

#[test]
fn sentinel_update_versions_rearm_bucket_configuration() {
    let (_root, store, job) = fixture();
    let store = Arc::new(Mutex::new(store));
    let mut revision = update("Keep watching the same state");
    revision.rearm_on_change.insert("done".into());

    let amended = apply(&store, &job, 1, "creator", revision).expect("definition update");
    let history = history(store.lock().get(&job.id).expect("definition history"));

    assert!(amended.rearm_on_change.contains("done"));
    assert!(history.versions[1]
        .definition
        .rearm_on_change
        .contains("done"));
    assert!(!history.versions[0]
        .definition
        .rearm_on_change
        .contains("done"));
}

fn apply(
    store: &Arc<Mutex<Store>>,
    current: &Sentinel,
    expect_version: u64,
    author: &str,
    update: Update,
) -> Result<Sentinel, String> {
    let prepared = update.prepare(current, author, true)?;
    crate::storage::commit(store, |store| {
        store.stage_update_definition(expect_version, author, 2000, prepared)
    })
}

#[test]
fn sentinel_update_expect_version_conflict_leaves_definition_unchanged() {
    let (_root, store, job) = fixture();
    let store = Arc::new(Mutex::new(store));
    let before = serde_json::to_value(store.lock().get(&job.id).unwrap()).unwrap();

    let error = apply(&store, &job, 9, "editor", update("New question?")).unwrap_err();

    assert_eq!(error, "blueprint_version_conflict: current_version=1");
    let after = serde_json::to_value(store.lock().get(&job.id).unwrap()).unwrap();
    assert_eq!(after, before);
}

#[test]
fn sentinel_update_appends_lineage_and_fences_the_in_flight_lease() {
    let (_root, store, job) = fixture();
    let store = Arc::new(Mutex::new(store));
    let lease = store.lock().due().unwrap().remove(0);

    let updated =
        apply(&store, &job, 1, "editor", update("New question?")).expect("amendment applies");

    assert_eq!(updated.definition_version, 2);
    assert_eq!(updated.revision, 1);
    assert_eq!(updated.amendments[0].replaced_version, 1);
    assert_eq!(updated.amendments[0].author_tab_id, "editor");
    assert!(lease.is_cancelled());
    assert!(!store.lock().finish(&lease.job).unwrap());
    let history = history(&updated);
    assert_eq!(history.current_version, 2);
    assert_eq!(history.versions.len(), 2);
    assert_eq!(history.versions[0].definition.question, "Is it done?");
    assert_eq!(history.versions[1].definition.question, "New question?");
    assert!(!history.history_truncated);
}

#[test]
fn sentinel_update_allows_a_scoped_non_creator_without_new_command_text() {
    let (_root, _store, job) = fixture();

    let prepared = update("New question?").prepare(&job, "editor", true);

    assert!(prepared.is_ok());
}

#[test]
fn sentinel_update_preview_preparation_does_not_change_the_stored_definition() {
    let (_root, store, job) = fixture();
    let before = serde_json::to_value(store.get(&job.id).unwrap()).unwrap();

    let preview = update("New question?")
        .prepare(&job, "editor", true)
        .unwrap();

    assert_eq!(preview.job.name.as_deref(), Some("updated"));
    assert_eq!(
        serde_json::to_value(store.get(&job.id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn sentinel_update_definition_requires_a_meaningful_name() {
    let (_root, _store, job) = fixture();
    let mut definition = update("New question?");
    definition.name = None;

    let error = definition.prepare(&job, "editor", true).err().unwrap();

    assert!(error.contains("Blueprint update definition needs a name"));
}

#[test]
fn sentinel_update_refuses_observe_rebind_and_non_creator_command_text() {
    let (_root, _store, job) = fixture();
    let observe = serde_json::json!({
        "name": "updated",
        "question": "Is it done?",
        "buckets": ["done"],
        "every": "30s",
        "routes": {"done": "notify"},
        "matches": {"done": "done"},
        "until": null,
        "on_other": [],
        "max_transitions": 64,
        "max_actions": 256,
        "observe": {"kind": "tab", "id": "another-tab"}
    });
    assert!(serde_json::from_value::<Update>(observe).is_err());

    let mut command_update = update("Is it done?");
    command_update.routes.clear();
    command_update.handlers = Some(BTreeMap::from([(
        "done".into(),
        vec![Definition {
            id: "new-command".into(),
            kind: "command".into(),
            parameters: handlers::Parameters {
                target: None,
                command: Some("echo new command".into()),
                label: Some("New command".into()),
                timeout_ms: Some(1000),
            },
        }],
    )]));
    assert!(command_update.prepare(&job, "editor", true).is_err());
}

#[test]
fn sentinel_update_keeps_bounded_immutable_json_history() {
    let (_root, store, first) = fixture();
    let store = Arc::new(Mutex::new(store));
    let mut current = first;
    for version in 1..=18 {
        current = apply(
            &store,
            &current,
            version,
            "editor",
            update(&format!("question {version}")),
        )
        .unwrap();
    }
    let history = history(&current);

    assert_eq!(current.amendments.len(), MAX_AMENDMENTS);
    assert_eq!(history.versions.len(), MAX_AMENDMENTS + 1);
    assert!(history.history_truncated);
    assert_eq!(
        history.versions.last().unwrap().version,
        current.definition_version
    );
}
