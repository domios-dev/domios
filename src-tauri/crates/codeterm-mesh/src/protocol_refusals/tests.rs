use super::*;

#[test]
fn a_polling_old_peer_reports_refusal_once_until_it_is_accepted() {
    let mut refusals = ProtocolRefusals::default();

    assert_eq!(
        refusals.observe("old", true),
        Some(ProtocolRefusalTransition::Refused)
    );
    for _ in 0..1000 {
        assert_eq!(refusals.observe("old", true), None);
    }
    assert_eq!(
        refusals.observe("old", false),
        Some(ProtocolRefusalTransition::Accepted)
    );
    assert_eq!(refusals.observe("old", false), None);
    assert_eq!(
        refusals.observe("old", true),
        Some(ProtocolRefusalTransition::Refused)
    );
}

#[test]
fn accepted_peers_never_report_and_peers_are_independent() {
    let mut refusals = ProtocolRefusals::default();

    assert_eq!(refusals.observe("current", false), None);
    assert_eq!(
        refusals.observe("a", true),
        Some(ProtocolRefusalTransition::Refused)
    );
    assert_eq!(
        refusals.observe("b", true),
        Some(ProtocolRefusalTransition::Refused)
    );
    assert_eq!(refusals.observe("a", true), None);
}

#[test]
fn forgetting_an_unpaired_peer_drops_its_state_silently() {
    let mut refusals = ProtocolRefusals::default();
    refusals.observe("old", true);

    refusals.forget("old");

    assert_eq!(refusals.observe("old", false), None);
}
