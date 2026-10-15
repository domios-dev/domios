use super::*;

#[test]
fn detected_prompt_is_answered_once_then_session_evidence_stops_the_attempt() {
    let mut attempt = Attempt::default();
    assert!(!attempt.observe(false, false));
    assert!(attempt.observe(false, true));
    assert!(!attempt.observe(false, true));
    assert!(!attempt.confirmed);
    assert!(!attempt.observe(true, true));
    assert!(attempt.confirmed);
    assert!(!attempt.observe(false, true));
}

#[test]
fn absent_prompt_never_authorizes_a_key() {
    let mut attempt = Attempt::default();
    for _ in 0..8 {
        assert!(!attempt.observe(false, false));
    }
    assert!(!attempt.answered);
    assert!(!attempt.confirmed);
    assert!(!attempt.observe(true, false));
    assert!(attempt.confirmed);
}

#[test]
fn evidence_before_prompt_sends_nothing() {
    let mut attempt = Attempt::default();
    assert!(!attempt.observe(true, true));
    assert!(!attempt.answered);
    assert!(attempt.confirmed);
}
