use super::*;

fn frame(text: &str, kind: Kind) -> Frame { Frame { bytes: Bytes::copy_from_slice(text.as_bytes()), kind } }
fn put(model: &mut Model, id: &str, text: &str, kind: Kind) { assert!(model.push(id, frame(text, kind)).is_ok()); }

#[test]
fn interactive_precedes_bulk_without_starving_bulk_or_reordering_a_channel() {
    let mut model = Model::default();
    model.register("bulk".into(), Traffic::Bulk, false);
    model.register("interactive".into(), Traffic::Interactive, false);
    put(&mut model, "bulk", "head", Kind::Head);
    put(&mut model, "bulk", "body", Kind::Data);
    put(&mut model, "bulk", "end", Kind::End);
    assert_eq!(model.pop().unwrap(), "head");
    for _ in 0..32 {
        put(&mut model, "interactive", "rpc", Kind::Data);
        let bytes = model.pop().unwrap();
        if bytes == "body" { assert_eq!(model.pop().unwrap(), "rpc"); assert_eq!(model.pop().unwrap(), "end"); return; }
    }
    panic!("bulk was starved");
}

#[test]
fn bulk_channels_rotate_one_frame_at_a_time() {
    let mut model = Model::default();
    for id in ["a", "b"] { model.register(id.into(), Traffic::Bulk, false); put(&mut model, id, id, Kind::Data); put(&mut model, id, id, Kind::Data); }
    for expected in ["a", "b", "a", "b"] { assert_eq!(model.pop().unwrap(), expected); }
}

#[test]
fn bounds_backpressure_rpc_and_overflow_streams_then_cancel_releases_bytes() {
    let mut model = Model::default();
    model.register("stream".into(), Traffic::Bulk, true);
    put(&mut model, "stream", "head", Kind::Head);
    for _ in 1..CHANNEL_DEPTH { put(&mut model, "stream", "data", Kind::Data); }
    assert_eq!(model.push("stream", frame("full", Kind::Data)).unwrap_err().0, Push::Overflow);
    model.finish_overflow("stream", Bytes::from_static(b"capacity"));
    assert_eq!(model.pop().unwrap(), "head");
    assert_eq!(model.pop().unwrap(), "capacity");
    assert_eq!(model.buffered, 0);
    model.register("rpc".into(), Traffic::Bulk, false);
    for _ in 0..CHANNEL_DEPTH { put(&mut model, "rpc", "data", Kind::Data); }
    assert_eq!(model.push("rpc", frame("full", Kind::Data)).unwrap_err().0, Push::Wait);
    model.remove("rpc");
    assert_eq!(model.buffered, 0);
}

#[test]
fn session_byte_budget_reserves_interactive_space_and_refuses_oversized_frames() {
    let mut model = Model::default();
    for n in 0..64 {
        let id = n.to_string(); model.register(id.clone(), Traffic::Bulk, false);
        for _ in 0..CHANNEL_DEPTH {
            if model.push(&id, Frame { bytes: Bytes::from(vec![0; FRAME_CAP]), kind: Kind::Data }).is_err() { break; }
        }
    }
    assert!(model.buffered <= BYTE_BUDGET - INTERACTIVE_RESERVE);
    model.register("rpc".into(), Traffic::Interactive, false);
    put(&mut model, "rpc", "priority", Kind::Data);
    assert_eq!(model.pop().unwrap(), "priority");
    assert_eq!(model.push("rpc", Frame { bytes: Bytes::from(vec![0; FRAME_CAP+1]), kind: Kind::Data }).unwrap_err().0, Push::Oversized);
    model.close(); assert_eq!(model.buffered, 0); assert!(model.pop().is_none());
}
