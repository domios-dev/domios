use super::effect_after_claim;

#[test]
fn blueprint_pipeline_effect_runs_after_one_successful_claim() {
    let mut events = Vec::new();
    let result = effect_after_claim(
        || {
            events.push("claim");
            true
        },
        || {
            events.push("effect");
            7
        },
    );

    assert_eq!(result, Some(7));
    assert_eq!(events, ["claim", "effect"]);
}

#[test]
fn blueprint_pipeline_refused_claim_skips_the_effect() {
    let mut events = Vec::new();
    let result = effect_after_claim(
        || {
            events.push("claim");
            false
        },
        || {
            events.push("effect");
        },
    );

    assert_eq!(result, None);
    assert_eq!(events, ["claim"]);
}
