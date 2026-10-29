use super::Readiness;

fn readiness(listener: bool, gate: bool) -> Readiness {
    Readiness {
        listeners: listener
            .then(|| "100.64.0.1:7685".parse().unwrap())
            .into_iter()
            .collect(),
        peers: (listener && gate)
            .then(|| "peer".to_string())
            .into_iter()
            .collect(),
    }
}

#[test]
fn startup_before_readiness_announces_once_more_in_either_order() {
    for listener_first in [false, true] {
        let mut state = readiness(false, false);
        let mut broadcasts = 1;
        broadcasts += usize::from(state.update(readiness(listener_first, !listener_first)));
        assert_eq!(broadcasts, 1);
        broadcasts += usize::from(state.update(readiness(true, true)));
        assert_eq!(broadcasts, 2);
    }
}

#[test]
fn repeated_readiness_does_not_announce() {
    let mut state = readiness(true, true);
    assert!(!state.update(readiness(true, true)));
    assert!(!state.update(readiness(true, true)));
}

#[test]
fn gate_drop_and_recovery_announces_once() {
    let mut state = readiness(true, true);
    assert!(!state.update(readiness(true, false)));
    assert!(state.update(readiness(true, true)));
    assert!(!state.update(readiness(true, true)));
}

#[test]
fn listener_drop_and_recovery_each_announce_once() {
    let mut state = readiness(true, true);
    assert!(state.update(readiness(false, true)));
    assert!(!state.update(readiness(false, true)));
    assert!(state.update(readiness(true, true)));
    assert!(!state.update(readiness(true, true)));
}

#[test]
fn another_peer_becomes_ready() {
    let mut state = readiness(true, true);
    let mut next = readiness(true, true);
    next.peers.insert("another-peer".into());
    assert!(state.update(next));
}
