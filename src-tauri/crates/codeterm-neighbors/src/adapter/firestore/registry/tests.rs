use super::super::value;
use super::*;

#[test]
fn message_read_signal_has_an_exact_shape_and_safe_timestamp() {
    let mut doc = document("own-account-signal");
    doc.fields.insert("type".into(), string("message_read"));
    doc.fields.insert("message_id".into(), string(&"c".repeat(32)));
    doc.fields.insert("at".into(), value(fs::value::ValueType::IntegerValue(1000)));
    let signal = parse_signal(doc.clone()).unwrap();
    assert_eq!(signal.read, Some(crate::registry::ReadSignal { friendship_id:"b".repeat(64), message_id:"c".repeat(32), read_at:1000 }));
    assert!(signal.delivery.is_none());
    for key in ["schema_version","revision","event_id","type","request_id","friendship_id","message_id","at"] {
        let mut invalid = doc.clone();
        invalid.fields.remove(key);
        assert!(parse_signal(invalid).is_err(), "missing {key}");
    }
    for (key, wrong) in [
        ("at",value(fs::value::ValueType::IntegerValue(-1))),
        ("at",value(fs::value::ValueType::IntegerValue(crate::wire::MAX_SAFE_INTEGER as i64 + 1))),
        ("at",string("1000")),
        ("message_id",string(&"C".repeat(32))),
        ("request_id",string(&"d".repeat(64))),
        ("friendship_id",value(fs::value::ValueType::NullValue(0))),
        ("unexpected",string("opaque")),
    ] {
        let mut invalid = doc.clone();
        invalid.fields.insert(key.into(),wrong);
        assert!(parse_signal(invalid).is_err(), "invalid {key}");
    }
}

#[test]
fn pause_changed_requires_the_exact_own_account_signal_shape() {
    let mut doc = document("own-account-signal");
    doc.fields.insert("type".into(), string("pause_changed"));
    doc.fields
        .insert("at".into(), value(fs::value::ValueType::IntegerValue(100)));
    for paused in [true, false] {
        doc.fields.insert(
            "paused".into(),
            value(fs::value::ValueType::BooleanValue(paused)),
        );
        let signal = parse_signal(doc.clone()).unwrap();
        assert_eq!(
            signal.pause,
            Some(crate::registry::PauseSignal {
                friendship_id: "b".repeat(64),
                paused
            })
        );
        assert!(signal.delivery.is_none());
        assert!(signal.removed.is_empty());
    }
    for (key, wrong) in [
        ("paused", string("true")),
        ("friendship_id", value(fs::value::ValueType::NullValue(0))),
        ("request_id", string(&"a".repeat(64))),
    ] {
        let mut invalid = doc.clone();
        invalid.fields.insert(key.into(), wrong);
        assert!(parse_signal(invalid).is_err());
    }
}

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
    let mut removed = document(&path);
    removed
        .fields
        .insert("type".into(), string("friend_removed"));
    assert_eq!(parse_signal(removed).unwrap().removed, vec!["b".repeat(64)]);
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
    assert!(parse_signal(malformed).unwrap().ignored);
}

#[test]
fn delivery_signals_validate_identity_timestamp_and_terminal_status() {
    for (kind, expected) in [
        ("message_delivered", crate::wire::DeliveryStatus::Delivered),
        ("message_expired", crate::wire::DeliveryStatus::NotDelivered),
    ] {
        let mut doc = document("own");
        doc.fields.insert("type".into(), string(kind));
        doc.fields
            .insert("message_id".into(), string(&"a".repeat(32)));
        doc.fields.insert(
            "at".into(),
            value(fs::value::ValueType::IntegerValue(1793130000)),
        );
        let signal = parse_signal(doc.clone()).unwrap();
        assert_eq!(signal.revision, Some(2));
        let delivery = signal.delivery.unwrap();
        assert_eq!(delivery.friendship_id, "b".repeat(64));
        assert_eq!(delivery.message_id, "a".repeat(32));
        assert_eq!(delivery.status, expected);
        assert!(signal.accepted.is_empty());
        let mut missing_time = doc.clone();
        missing_time.fields.remove("at");
        assert!(parse_signal(missing_time).is_err());
        doc.fields.insert("message_id".into(), string("bad"));
        assert!(parse_signal(doc).is_err());
    }
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
