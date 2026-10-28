use super::*;
use serde_json::json;

fn fixture_history(direction: &str) -> History {
    serde_json::from_value(json!({"ended":false,"entries":(1..=3).map(|id| json!({"direction":direction,"author":"me","sender_uid":"owner","body":"opaque","at":id,"message_id":format!("{id:032x}"),"kind":"question","submission":"accepted","delivery_status":"pending"})).collect::<Vec<_>>()})).unwrap()
}

#[test]
fn read_up_to_is_monotonic_and_reserves_one_receipt_for_each_new_batch() {
    let mut history = fixture_history("in");
    let emitted = [2, 2, 1, 3, 2]
        .into_iter()
        .filter(|id| history.reserve_read_through(&format!("{id:032x}"), true, false))
        .count();
    assert_eq!(emitted, 2);
    let bytes = serde_json::to_vec(&history).unwrap();
    let mut restored: History = serde_json::from_slice(&bytes).unwrap();
    assert!(!restored.reserve_read_through(&format!("{:032x}", 3), true, false));
    assert!(!restored.reserve_read_through(&"f".repeat(32), true, false));
    let mut bounded = fixture_history("in");
    assert!(bounded.reserve_read_through(&format!("{:032x}", 1), true, false));
    bounded.entries.remove(0);
    assert!(bounded.reserve_read_through(&format!("{:032x}", 3), true, false));
}

#[test]
fn off_paused_and_ended_friendships_never_reserve_a_read_receipt() {
    for (enabled, paused, ended) in [
        (false, false, false),
        (true, true, false),
        (true, false, true),
    ] {
        let mut history = fixture_history("in");
        history.ended = ended;
        assert!(!history.reserve_read_through(&format!("{:032x}", 3), enabled, paused));
        assert!(history.read_receipt_through.is_none());
    }
    assert!(!fixture_history("out").reserve_read_through(&format!("{:032x}", 3), true, false));
}

#[test]
fn concierge_read_requires_pickup_or_a_terminal_reply_outcome() {
    for (state, read) in [
        (State::Waiting, false),
        (State::DeliveredToConcierge, false),
        (State::Failed, false),
        (State::Stopped, false),
        (State::Working, true),
        (State::Answered, true),
        (State::Refused, true),
        (State::NoReply, true),
    ] {
        let mut history = fixture_history("in");
        history.entries[1].handling.handling_state = Some(state);
        assert_eq!(
            history.concierge_read_through(),
            read.then(|| format!("{:032x}", 2))
        );
    }
}

#[test]
fn read_signal_marks_only_own_outgoing_prefix_and_never_regresses_timestamps() {
    let mut history = fixture_history("out");
    history.entries[0].delivered_at = Some(100);
    assert!(!history.apply_read_through("foreign", &format!("{:032x}", 2), 200));
    assert!(history.apply_read_through("owner", &format!("{:032x}", 2), 200));
    assert_eq!(history.entries[0].delivered_at, Some(100));
    assert_eq!(history.entries[1].delivered_at, Some(200));
    assert_eq!(history.entries[1].read_at, Some(200));
    assert!(history.entries[2].read_at.is_none());
    assert!(!history.apply_read_through("owner", &format!("{:032x}", 1), 300));
    assert_eq!(history.entries[0].read_at, Some(200));
}
