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
    assert!(published.publish(3, &json!({ "generation": 3 })).unwrap());
    release_tx.send(()).unwrap();
    assert!(!worker.join().unwrap());
    assert_eq!(published.read(), r#"{"generation":3}"#);
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
