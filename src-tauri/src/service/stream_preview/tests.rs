use super::*;

fn grid(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|r| r.to_string()).collect()
}

#[test]
fn no_events_before_any_turn_started() {
    let pty = "sp-test-no-turn";
    assert!(step(pty, "tab", &grid(&["hello", ""]), None).is_none());
    assert!(step(pty, "tab", &grid(&["hello", "world"]), None).is_none());
    remove(pty);
}

#[test]
fn emits_on_change_and_gates_identical_frames() {
    let pty = "sp-test-gate";
    step(pty, "tab", &grid(&["❯", "", "", ""]), None);
    begin_turn(pty);
    let first = step(pty, "tab", &grid(&["❯", "reply starts", "", ""]), None).expect("turn view changed");
    assert_eq!(first.turn_seq, 1);
    assert_eq!(first.live, vec!["reply starts"]);
    assert!(
        step(pty, "tab", &grid(&["❯", "reply starts", "", ""]), None).is_none(),
        "identical view is change-gated"
    );
    let grown =
        step(pty, "tab", &grid(&["❯", "reply starts", "and grows", ""]), None).expect("view grew");
    assert_eq!(grown.live, vec!["reply starts", "and grows"]);
    remove(pty);
}

#[test]
fn frozen_rows_accumulate_across_scrolls() {
    let pty = "sp-test-frozen";
    step(pty, "tab", &grid(&["pre one", "pre two", "pre three", "", "", ""]), None);
    begin_turn(pty);
    step(pty, "tab", &grid(&["pre one", "pre two", "pre three", "turn a", "turn b", "turn c"]), None);
    step(pty, "tab", &grid(&["pre three", "turn a", "turn b", "turn c", "turn d", "turn e"]), None);
    let event = step(
        pty,
        "tab",
        &grid(&["turn a", "turn b", "turn c", "turn d", "turn e", "turn f"]),
        None,
    )
    .expect("scroll changed the view");
    assert!(event.frozen.is_empty(), "pre-turn rows never freeze");
    let event = step(
        pty,
        "tab",
        &grid(&["turn c", "turn d", "turn e", "turn f", "turn g", "turn h"]),
        None,
    )
    .expect("turn rows froze");
    assert_eq!(event.frozen, vec!["turn a", "turn b"]);
    assert!(event.live.starts_with(&["turn c".to_string()]));
    remove(pty);
}

#[test]
fn new_turn_resets_frozen_history() {
    let pty = "sp-test-reset";
    step(pty, "tab", &grid(&["one", "two", "three", "four", "five", "six"]), None);
    begin_turn(pty);
    step(pty, "tab", &grid(&["one", "two", "three", "four", "five", "turn x"]), None);
    begin_turn(pty);
    let event = step(pty, "tab", &grid(&["one", "two", "three", "four", "five", "next turn"]), None)
        .expect("new turn view");
    assert_eq!(event.turn_seq, 2);
    assert!(event.frozen.is_empty());
    assert_eq!(event.live, vec!["next turn"]);
    remove(pty);
}
