use super::*;
use serde::Serializer;
use serde_json::json;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn first_read_has_a_complete_snapshot_and_shares_its_buffer() {
    let initial = json!({ "generation": 1, "tabs": {} });
    let published = PublishedMuxState::new(1, &initial).unwrap();
    let first = published.read();
    let second = published.read();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&first).unwrap(),
        initial
    );
    assert_eq!(first.as_ptr(), second.as_ptr());
}

struct PausedSnapshot {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    value: serde_json::Value,
}

impl Serialize for PausedSnapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(2)).unwrap();
        self.value.serialize(serializer)
    }
}

#[test]
fn rebuild_does_not_block_readers_and_late_old_completion_cannot_roll_back() {
    let published = Arc::new(PublishedMuxState::new(1, &json!({ "generation": 1 })).unwrap());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_state = published.clone();
    let worker = std::thread::spawn(move || {
        worker_state
            .publish(
                2,
                &PausedSnapshot {
                    entered: entered_tx,
                    release: release_rx,
                    value: json!({ "generation": 2 }),
                },
            )
            .unwrap()
    });
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(published.read(), r#"{"generation":1}"#);
    assert!(published
        .publish(3, &json!({ "generation": 3, "title": "newest" }))
        .unwrap());
    release_tx.send(()).unwrap();
    assert!(!worker.join().unwrap());
    assert_eq!(published.read(), r#"{"generation":3,"title":"newest"}"#);
}

#[test]
fn unchanged_snapshot_is_silent_and_changed_snapshot_publishes_at_once() {
    let published =
        PublishedMuxState::new(1, &json!({ "generation": 1, "title": "first" })).unwrap();
    for generation in 2..=50 {
        assert!(!published
            .publish(
                generation,
                &json!({ "generation": generation, "title": "first" })
            )
            .unwrap());
    }
    assert_eq!(
        published.read_versioned(),
        (
            1,
            Bytes::from_static(br#"{"generation":1,"title":"first"}"#)
        )
    );
    assert!(published
        .publish(51, &json!({ "generation": 51, "title": "second" }))
        .unwrap());
    assert_eq!(
        published.read_versioned(),
        (
            51,
            Bytes::from_static(br#"{"generation":51,"title":"second"}"#)
        )
    );
}

#[test]
fn silent_newer_generation_still_fences_an_older_late_publication() {
    let published = PublishedMuxState::new(1, &json!({ "generation": 1, "title": "a" })).unwrap();
    assert!(!published
        .publish(3, &json!({ "generation": 3, "title": "a" }))
        .unwrap());
    assert!(!published
        .publish(2, &json!({ "generation": 2, "title": "stale" }))
        .unwrap());
    assert_eq!(published.read(), r#"{"generation":1,"title":"a"}"#);
}

#[test]
fn equal_multiplexer_snapshots_publish_once_whatever_their_map_order() {
    use crate::multiplexer::state::MultiplexerState;
    use codeterm_types::pty::TabStatus;

    let (mut state, first) = MultiplexerState::new();
    let workspace = state.list_workspaces()[0].id.clone();
    let mut tabs = vec![first];
    for _ in 0..12 {
        tabs.push(state.create_tab(&workspace).unwrap());
    }
    let snapshot = |state: &MultiplexerState| {
        let mut snapshot = state.snapshot();
        snapshot.tab_statuses = tabs
            .iter()
            .map(|tab| (tab.clone(), TabStatus::Working))
            .collect();
        snapshot
    };
    let initial = snapshot(&state);
    let published = PublishedMuxState::new(initial.generation, &initial).unwrap();
    for _ in 0..20 {
        let unchanged = snapshot(&state);
        assert!(!published.publish(unchanged.generation, &unchanged).unwrap());
    }
    state.rename_tab(&tabs[3], "Renamed").unwrap();
    let renamed = snapshot(&state);
    assert!(published.publish(renamed.generation, &renamed).unwrap());
    assert_eq!(published.read_versioned().0, renamed.generation);
}

#[test]
fn serialization_failure_keeps_the_previous_snapshot() {
    struct Failed;
    impl Serialize for Failed {
        fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("opaque failure"))
        }
    }
    let published = PublishedMuxState::new(1, &json!({ "generation": 1 })).unwrap();
    assert!(published.publish(2, &Failed).is_err());
    assert_eq!(published.read(), r#"{"generation":1}"#);
}

#[test]
fn versioned_read_pairs_generation_with_the_same_published_body() {
    let published = PublishedMuxState::new(4, &json!({"generation": 4})).unwrap();
    let (generation, json) = published.read_versioned();
    assert_eq!(generation, 4);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json).unwrap()["generation"],
        generation
    );
    assert!(published
        .publish(5, &json!({"generation": 5, "title": "changed"}))
        .unwrap());
    let (generation, json) = published.read_versioned();
    assert_eq!(generation, 5);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json).unwrap()["generation"],
        generation
    );
}
