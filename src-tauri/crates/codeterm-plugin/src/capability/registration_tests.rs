use super::*;
#[test]
fn capability_registration_prepares_initial_and_reloaded_implementations() {
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let seen = events.clone();
    let registry = CapabilityRegistry::<String>::with_registration_hook(move |id, value| {
        seen.lock().push((id.to_owned(), value.as_str().to_owned()))
    });
    registry.register("fixture", Arc::new("first".into()));
    registry.register("fixture", Arc::new("second".into()));
    assert_eq!(
        *events.lock(),
        vec![
            ("fixture".into(), "first".into()),
            ("fixture".into(), "second".into())
        ]
    );
    assert_eq!(registry.get("fixture").unwrap().as_str(), "second");
}
