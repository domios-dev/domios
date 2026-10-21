use super::*;
#[test]
fn failing_observation_preserves_done_route_unless_explicitly_overridden() {
    let mut step: Step = serde_json::from_value(serde_json::json!({"name":"CI", "input":"text","kind":{"kind":"github","probe":{"repo":"owner/repo","resource":"run","id":42}}})).unwrap();
    let reading = || SourceReading { state:"failing".into(), summary:"failing · Linux".into(), output:serde_json::json!({"source_state":"failing", "failed_jobs":["Linux"]}) };
    let result = reading_result(&step, reading()); assert_eq!(result.outcome, StepOutcome::Done); assert_eq!(result.stored_outcome, "failing"); assert!(result.next_text.contains("Linux"));
    step.on.insert("failing".into(), "stop".into());
    assert_eq!(reading_result(&step, reading()).outcome, StepOutcome::Bucket("failing".into()));
}
#[test]
fn health_observation_routes_down_and_retains_status_output() {
    let step: Step = serde_json::from_value(serde_json::json!({"name":"health", "input":"text","kind":{"kind":"http","probe":{"url":"https://example.com"}}})).unwrap();
    let result = reading_result(&step, SourceReading { state:"down".into(), summary:"down · 503".into(), output:serde_json::json!({"state":"down","code":503}) });
    assert_eq!(result.outcome, StepOutcome::Bucket("down".into())); assert_eq!(result.stored_outcome, "down"); assert_eq!(result.state, "done"); assert_eq!(serde_json::from_str::<serde_json::Value>(&result.next_text).unwrap()["code"], 503);
}
