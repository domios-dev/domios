use super::*;
use crate::agent::test_provider::MockProvider;

#[test]
fn registration_reload_and_removal_publish_after_registry_changes() {
    let registry = Arc::new(AgentRegistry::new());
    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let weak = Arc::downgrade(&registry);
    let recorded = events.clone();
    registry.observe_provider_changes(Arc::new(move |id| {
        let current = weak.upgrade().unwrap();
        recorded
            .lock()
            .push((id.to_owned(), current.provider_arc(id).is_some()));
    }));
    registry.register_provider(Box::new(MockProvider::new("reload_fixture")));
    registry.register_provider(Box::new(MockProvider::new("reload_fixture")));
    assert!(registry.unregister_provider("reload_fixture"));
    assert_eq!(
        *events.lock(),
        vec![
            ("reload_fixture".into(), true),
            ("reload_fixture".into(), true),
            ("reload_fixture".into(), false),
        ]
    );
}
