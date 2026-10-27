use super::super::value;
use super::*;

fn string(text: &str) -> fs::Value {
    value(fs::value::ValueType::StringValue(text.into()))
}

fn document(path: &str) -> fs::Document {
    fs::Document {
        name: path.into(),
        fields: [
            (
                "schema_version",
                value(fs::value::ValueType::IntegerValue(1)),
            ),
            ("revision", value(fs::value::ValueType::IntegerValue(2))),
            ("event_id", string(&"e".repeat(32))),
            ("type", string("request_accepted")),
            ("request_id", value(fs::value::ValueType::NullValue(0))),
            ("friendship_id", string(&"b".repeat(64))),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect(),
        ..Default::default()
    }
}

#[test]
fn own_signal_snapshot_refreshes_at_current_and_rejects_foreign_documents() {
    let path = document_path("projects/fixture/databases/(default)", "owner").unwrap();
    assert!(path.ends_with("/neighborSignals/owner"));
    assert!(document_path("database", "other/path").is_err());
    let mut state = SignalState {
        path: path.clone(),
        pending: RegistrySignal::default(),
        dirty: false,
    };
    let response = |doc| fs::ListenResponse {
        response_type: Some(fs::listen_response::ResponseType::DocumentChange(
            fs::DocumentChange {
                document: Some(doc),
                target_ids: vec![1],
                ..Default::default()
            },
        )),
    };
    assert!(state.observe(response(document(&path))).unwrap().is_none());
    let checkpoint = fs::ListenResponse {
        response_type: Some(fs::listen_response::ResponseType::TargetChange(
            fs::TargetChange {
                target_change_type: fs::target_change::TargetChangeType::Current as i32,
                target_ids: vec![1],
                ..Default::default()
            },
        )),
    };
    let signal = state.observe(checkpoint).unwrap().unwrap();
    assert_eq!(signal.accepted.len(), 1);
    assert_eq!(signal.accepted[0].event_id, "e".repeat(32));
    assert_eq!(signal.accepted[0].friendship_id, "b".repeat(64));
    assert!(matches!(
        state.observe(response(document("foreign"))),
        Err(Error::WrongPeer)
    ));
    let mut accepter = document(&path);
    accepter
        .fields
        .insert("request_id".into(), string(&"a".repeat(64)));
    assert!(parse_signal(accepter).unwrap().accepted.is_empty());
    let mut invalid_event = document(&path);
    invalid_event
        .fields
        .insert("event_id".into(), string("event"));
    assert!(parse_signal(invalid_event).is_err());
}

#[test]
fn empty_signal_target_refreshes_once_and_does_not_poll_unchanged_checkpoints() {
    let mut state = SignalState {
        path: "own".into(),
        pending: RegistrySignal::default(),
        dirty: false,
    };
    let checkpoint = |kind| fs::ListenResponse {
        response_type: Some(fs::listen_response::ResponseType::TargetChange(
            fs::TargetChange {
                target_change_type: kind as i32,
                target_ids: vec![1],
                read_time: Some(prost_types::Timestamp {
                    seconds: 1,
                    nanos: 0,
                }),
                ..Default::default()
            },
        )),
    };
    assert!(state
        .observe(checkpoint(fs::target_change::TargetChangeType::Current))
        .unwrap()
        .is_some());
    assert!(state
        .observe(checkpoint(fs::target_change::TargetChangeType::NoChange))
        .unwrap()
        .is_none());
    let mut malformed = document("own");
    malformed.fields.insert("type".into(), string("unknown"));
    assert!(parse_signal(malformed).is_err());
}

#[test]
fn receiver_changed_signal_keeps_revision_without_a_friend_acceptance() {
    let mut doc = document("own");
    doc.fields.insert("type".into(), string("receiver_changed"));
    doc.fields.insert(
        "friendship_id".into(),
        value(fs::value::ValueType::NullValue(0)),
    );
    assert!(parse_signal(doc.clone()).is_err());
    doc.fields.insert(
        "receiver_revision".into(),
        value(fs::value::ValueType::IntegerValue(8)),
    );
    let signal = parse_signal(doc).unwrap();
    assert_eq!(signal.receiver_revision, Some(8));
    assert!(signal.accepted.is_empty());
}
