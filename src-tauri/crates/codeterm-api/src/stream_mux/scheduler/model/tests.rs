use super::*;

fn frame(text: &str, kind: Kind) -> Frame {
    Frame {
        bytes: Bytes::copy_from_slice(text.as_bytes()),
        kind,
        credit: 0,
    }
}
/// A credited bulk data frame costing `credit` payload bytes.
fn credited(model: &mut Model, id: &str, text: &str, credit: u64) {
    let frame = Frame {
        bytes: Bytes::copy_from_slice(text.as_bytes()),
        kind: Kind::Data,
        credit,
    };
    assert!(model.push(id, frame).is_ok());
}
fn put(model: &mut Model, id: &str, text: &str, kind: Kind) {
    assert!(model.push(id, frame(text, kind)).is_ok());
}

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
        if bytes == "body" {
            assert_eq!(model.pop().unwrap(), "rpc");
            assert_eq!(model.pop().unwrap(), "end");
            return;
        }
    }
    panic!("bulk was starved");
}

#[test]
fn bulk_channels_rotate_one_frame_at_a_time() {
    let mut model = Model::default();
    for id in ["a", "b"] {
        model.register(id.into(), Traffic::Bulk, false);
        put(&mut model, id, id, Kind::Data);
        put(&mut model, id, id, Kind::Data);
    }
    for expected in ["a", "b", "a", "b"] {
        assert_eq!(model.pop().unwrap(), expected);
    }
}

#[test]
fn bounds_backpressure_rpc_and_overflow_streams_then_cancel_releases_bytes() {
    let mut model = Model::default();
    model.register("stream".into(), Traffic::Bulk, true);
    put(&mut model, "stream", "head", Kind::Head);
    for _ in 1..CHANNEL_DEPTH {
        put(&mut model, "stream", "data", Kind::Data);
    }
    assert_eq!(
        model
            .push("stream", frame("full", Kind::Data))
            .unwrap_err()
            .0,
        Push::Overflow
    );
    model.finish_overflow("stream", Bytes::from_static(b"capacity"));
    assert_eq!(model.pop().unwrap(), "head");
    assert_eq!(model.pop().unwrap(), "capacity");
    assert_eq!(model.buffered, 0);
    model.register("rpc".into(), Traffic::Bulk, false);
    for _ in 0..CHANNEL_DEPTH {
        put(&mut model, "rpc", "data", Kind::Data);
    }
    assert_eq!(
        model.push("rpc", frame("full", Kind::Data)).unwrap_err().0,
        Push::Wait
    );
    model.remove("rpc");
    assert_eq!(model.buffered, 0);
}

#[test]
fn session_byte_budget_reserves_interactive_space_and_refuses_oversized_frames() {
    let mut model = Model::default();
    for n in 0..64 {
        let id = n.to_string();
        model.register(id.clone(), Traffic::Bulk, false);
        for _ in 0..CHANNEL_DEPTH {
            if model
                .push(
                    &id,
                    Frame {
                        bytes: Bytes::from(vec![0; FRAME_CAP]),
                        kind: Kind::Data,
                        credit: 0,
                    },
                )
                .is_err()
            {
                break;
            }
        }
    }
    assert!(model.buffered <= BYTE_BUDGET - INTERACTIVE_RESERVE);
    model.register("rpc".into(), Traffic::Interactive, false);
    put(&mut model, "rpc", "priority", Kind::Data);
    assert_eq!(model.pop().unwrap(), "priority");
    assert_eq!(
        model
            .push(
                "rpc",
                Frame {
                    bytes: Bytes::from(vec![0; FRAME_CAP + 1]),
                    kind: Kind::Data,
                    credit: 0,
                }
            )
            .unwrap_err()
            .0,
        Push::Oversized
    );
    model.close();
    assert_eq!(model.buffered, 0);
    assert!(model.pop().is_none());
}

const QUARTER: u64 = BULK_WINDOW / 4;

#[test]
fn bulk_window_holds_bulk_until_credited_while_interactive_flows() {
    let mut model = Model::default();
    model.register("bulk".into(), Traffic::Bulk, false);
    model.register("interactive".into(), Traffic::Interactive, false);
    for _ in 0..4 {
        credited(&mut model, "bulk", "bulk", QUARTER);
        assert_eq!(model.pop().unwrap(), "bulk");
    }
    credited(&mut model, "bulk", "bulk", QUARTER);
    put(&mut model, "interactive", "rpc", Kind::Data);
    assert_eq!(model.pop().unwrap(), "rpc");
    assert!(
        model.pop().is_none(),
        "a full byte window holds bulk until credit"
    );
    model.credit(QUARTER);
    assert_eq!(model.pop().unwrap(), "bulk");
    assert!(model.pop().is_none());
}

#[test]
fn credit_is_cumulative_and_never_exceeds_bytes_sent() {
    let mut model = Model::default();
    model.register("bulk".into(), Traffic::Bulk, false);
    for _ in 0..4 {
        credited(&mut model, "bulk", "bulk", QUARTER);
        model.pop().unwrap();
    }
    model.credit(u64::MAX);
    model.credit(QUARTER);
    for _ in 0..4 {
        credited(&mut model, "bulk", "bulk", QUARTER);
        assert_eq!(model.pop().unwrap(), "bulk");
    }
    credited(&mut model, "bulk", "bulk", QUARTER);
    assert!(
        model.pop().is_none(),
        "an inflated credit must not open the window beyond bytes sent"
    );
}

#[test]
fn uncredited_frames_streams_and_terminal_frames_pass_a_full_window() {
    let mut model = Model::default();
    model.register("bulk".into(), Traffic::Bulk, false);
    model.register("stream".into(), Traffic::Bulk, true);
    credited(&mut model, "bulk", "bulk", BULK_WINDOW);
    model.pop().unwrap();
    put(&mut model, "bulk", "end", Kind::End);
    assert_eq!(model.pop().unwrap(), "end");
    put(&mut model, "stream", "frame", Kind::Data);
    assert_eq!(model.pop().unwrap(), "frame");
}

#[test]
fn cancelling_bulk_reads_mid_stream_never_wedges_the_window() {
    let mut model = Model::default();
    let mut received = 0;
    for read in 0..4 {
        let id = format!("cancelled-{read}");
        model.register(id.clone(), Traffic::Bulk, false);
        credited(&mut model, &id, "sent", QUARTER);
        credited(&mut model, &id, "queued", QUARTER);
        assert_eq!(model.pop().unwrap(), "sent");
        received += QUARTER;
        model.remove(&id);
    }
    assert_eq!(model.buffered, 0, "cancel releases queued bulk bytes");
    model.register("fresh".into(), Traffic::Bulk, false);
    credited(&mut model, "fresh", "body", QUARTER);
    put(&mut model, "fresh", "end", Kind::End);
    assert!(model.pop().is_none(), "only sent bytes fill the window");
    model.credit(received);
    assert_eq!(model.pop().unwrap(), "body");
    assert_eq!(model.pop().unwrap(), "end");
}

#[test]
fn an_interactive_end_frame_is_never_held_by_a_full_window() {
    let mut model = Model::default();
    model.register("bulk".into(), Traffic::Bulk, false);
    model.register("rpc".into(), Traffic::Interactive, false);
    credited(&mut model, "bulk", "fill", BULK_WINDOW);
    model.pop().unwrap();
    credited(&mut model, "bulk", "held", QUARTER);
    put(&mut model, "rpc", "head", Kind::Head);
    put(&mut model, "rpc", "end", Kind::End);
    assert_eq!(model.pop().unwrap(), "head");
    assert_eq!(model.pop().unwrap(), "end");
    assert!(model.pop().is_none());
}
