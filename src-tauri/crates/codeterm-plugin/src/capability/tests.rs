use super::*;

trait Greeter: Send + Sync {
    fn hi(&self) -> String;
}
struct A;
impl Greeter for A {
    fn hi(&self) -> String {
        "a".into()
    }
}
struct B;
impl Greeter for B {
    fn hi(&self) -> String {
        "b".into()
    }
}

#[test]
fn register_get_unregister() {
    let r: CapabilityRegistry<dyn Greeter> = CapabilityRegistry::new();
    assert!(r.get("a").is_none());
    r.register("a", Arc::new(A));
    assert_eq!(r.get("a").unwrap().hi(), "a");
    assert!(r.contains("a"));
    assert_eq!(r.active().len(), 1);
    assert_eq!(r.ids(), vec!["a".to_string()]);
    r.unregister("a");
    assert!(r.get("a").is_none());
    assert!(!r.contains("a"));
}

#[test]
fn register_replaces_same_id() {
    let r: CapabilityRegistry<dyn Greeter> = CapabilityRegistry::new();
    r.register("g", Arc::new(A));
    r.register("g", Arc::new(B));
    assert_eq!(r.get("g").unwrap().hi(), "b");
    assert_eq!(r.active().len(), 1);
}

#[test]
fn registration_ids_fence_replacement_and_removal_without_changing_lookup() {
    let registry: CapabilityRegistry<dyn Greeter> = CapabilityRegistry::new();
    assert!(registry.registration_id("fixture").is_none());
    registry.register("fixture", Arc::new(A));
    let first = registry.registration_id("fixture").unwrap();
    registry.register("fixture", Arc::new(B));
    assert_ne!(registry.registration_id("fixture"), Some(first));
    assert_eq!(registry.get("fixture").unwrap().hi(), "b");
    assert_eq!(registry.active().len(), 1);
    registry.unregister("fixture");
    assert!(registry.registration_id("fixture").is_none());
}
