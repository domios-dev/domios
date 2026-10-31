use super::*;

#[test]
fn durable_media_observation_survives_actual_fetch_worker_before_transport() {
    let _scope = physical::RequestScope::observation_only(
        std::time::Instant::now() + Duration::from_secs(2),
    );
    let result: serde_json::Value =
        serde_json::from_str(&run(r#"{"url":"https://example.invalid/"}"#)).unwrap();
    assert_eq!(result["error"], "decision request refused");
    assert!(!physical::effects_allowed());
}
