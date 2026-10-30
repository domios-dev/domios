use super::*;
use super::super::value;

fn response(kind: fs::listen_response::ResponseType) -> fs::ListenResponse { fs::ListenResponse { response_type: Some(kind) } }
fn checkpoint() -> fs::ListenResponse {
    response(fs::listen_response::ResponseType::TargetChange(fs::TargetChange {
        target_change_type: fs::target_change::TargetChangeType::Current as i32,
        target_ids: vec![1], ..Default::default()
    }))
}
fn field(input: Value) -> fs::Value {
    use fs::value::ValueType;
    value(match input {
        Value::Null => ValueType::NullValue(0),
        Value::Bool(value) => ValueType::BooleanValue(value),
        Value::Number(value) => ValueType::IntegerValue(value.as_i64().unwrap()),
        Value::String(value) => ValueType::StringValue(value),
        Value::Array(values) => ValueType::ArrayValue(fs::ArrayValue { values: values.into_iter().map(field).collect() }),
        Value::Object(fields) => ValueType::MapValue(fs::MapValue { fields: fields.into_iter().map(|(key,value)| (key,field(value))).collect() }),
    })
}

#[test]
fn missing_projection_can_arrive_on_the_same_listen_stream_and_foreign_paths_are_rejected() {
    let mut state = DocumentState { path: "own-document".into(), pending: None, missing: true };
    assert!(matches!(state.observe(checkpoint()).unwrap(), Some(StateEvent::Missing)));
    let document = serde_json::json!({"schema_version":1,"version":1,"updated_at":100,"access":"locked","can_invite":false,
        "friends":[],"incoming":[],"outgoing":[],"own_keys":[],
        "referral_trial":{"state":"none","trial_end":null,"inviter_uid":null,"friend_request_id":null}});
    let fields = document.as_object().unwrap().iter().map(|(key,value)| (key.clone(),field(value.clone()))).collect();
    let event = response(fs::listen_response::ResponseType::DocumentChange(fs::DocumentChange {
        document: Some(fs::Document { name: "own-document".into(), fields, ..Default::default() }), target_ids: vec![1], ..Default::default()
    }));
    assert!(state.observe(event).unwrap().is_none());
    assert!(matches!(state.observe(checkpoint()).unwrap(), Some(StateEvent::Snapshot(snapshot)) if snapshot.version == 1));
    let foreign = response(fs::listen_response::ResponseType::DocumentDelete(fs::DocumentDelete { document: "foreign".into(), ..Default::default() }));
    assert!(matches!(state.observe(foreign), Err(Error::WrongPeer)));
}

#[test]
fn decoder_bounds_arrays_depth_numbers_and_strings() {
    use fs::value::ValueType;
    assert!(decode(value(ValueType::IntegerValue(-1)),0).is_err());
    assert!(decode(value(ValueType::StringValue("x".repeat(2049))),0).is_err());
    assert!(decode(value(ValueType::NullValue(0)),6).is_err());
    assert!(decode(value(ValueType::ArrayValue(fs::ArrayValue { values: vec![value(ValueType::NullValue(0));201] })),0).is_err());
}
