use super::{identity, user_delivery_pair_has_same_generation, user_delivery_verdict_qualifies};
use codeterm_decision::{DecisionAnswer, DecisionResponse};
use std::collections::BTreeMap;

fn local_identity() -> identity::LocalIdentity {
    local_identity_for_nonce("nonce-restored")
}

fn local_identity_for_nonce(nonce: &str) -> identity::LocalIdentity {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-restored".into());
    tab.session_binding_nonce = Some(nonce.into());
    identity::LocalIdentity::of_tab(tab)
}

#[test]
fn supporter_fallback_requires_adapter_ready_idle_composer_and_no_prompt() {
    use identity::DecisionSource;

    assert!(user_delivery_verdict_qualifies(
        DecisionSource::Adapter,
        super::READY_IDLE,
        0.90,
        0.0,
        Some(("composer", 0.90)),
    ));
    assert!(!user_delivery_verdict_qualifies(
        DecisionSource::LowConfidence,
        super::READY_IDLE,
        0.99,
        0.0,
        Some(("composer", 0.99)),
    ));
    assert!(!user_delivery_verdict_qualifies(
        DecisionSource::Adapter,
        super::READY_IDLE,
        0.89,
        0.0,
        Some(("composer", 0.99)),
    ));
    assert!(!user_delivery_verdict_qualifies(
        DecisionSource::Adapter,
        super::READY_IDLE,
        0.99,
        0.11,
        Some(("composer", 0.99)),
    ));
    assert!(!user_delivery_verdict_qualifies(
        DecisionSource::Adapter,
        super::READY_IDLE,
        0.99,
        0.0,
        Some(("active_modal", 0.99)),
    ));
    assert!(!user_delivery_verdict_qualifies(
        DecisionSource::Adapter,
        super::READY_IDLE,
        0.99,
        0.0,
        Some(("composer", 0.89)),
    ));
}

#[test]
fn supporter_fallback_preserves_a_conflicting_prompt_probability() {
    let choice = |name: &str| DecisionAnswer::Choice {
        choice: name.to_string(),
        probabilities: BTreeMap::from([(name.to_string(), 0.99)]),
        confidence: Some(0.99),
    };
    let verdict = super::observer::observation_answers(DecisionResponse {
        answers: BTreeMap::from([
            ("label".into(), choice(super::READY_IDLE)),
            ("prompt_visible".into(), DecisionAnswer::Noul { p: 0.99 }),
            ("input_area".into(), choice("composer")),
        ]),
        usage: None,
    })
    .unwrap();

    assert_eq!(verdict.prompt_visible, 0.0);
    assert_eq!(verdict.reported_prompt_visible, 0.99);
    assert!(!user_delivery_verdict_qualifies(
        identity::DecisionSource::Adapter,
        &verdict.label,
        verdict.confidence,
        verdict.reported_prompt_visible,
        verdict
            .input_area
            .as_ref()
            .map(|(area, confidence)| (area.as_str(), *confidence)),
    ));
}

#[test]
fn supporter_fallback_requires_distinct_observations_of_one_launch_generation() {
    let local = local_identity();
    let same_launch = local.clone();
    let different_launch = local_identity_for_nonce("rotated-nonce");

    assert!(user_delivery_pair_has_same_generation(
        10,
        11,
        42,
        42,
        Some(&local),
        Some(&same_launch)
    ));
    assert!(!user_delivery_pair_has_same_generation(
        10,
        10,
        42,
        42,
        Some(&local),
        Some(&same_launch)
    ));
    assert!(!user_delivery_pair_has_same_generation(
        10,
        11,
        42,
        42,
        Some(&local),
        Some(&different_launch),
    ));
    assert!(!user_delivery_pair_has_same_generation(
        10,
        11,
        42,
        42,
        Some(&local),
        None
    ));
    assert!(!user_delivery_pair_has_same_generation(
        10,
        11,
        42,
        43,
        Some(&local),
        Some(&same_launch),
    ));
}
