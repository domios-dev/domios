use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use serde_json::Value;

use super::SecretStore;

pub trait SecretBackendFactory: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn build(&self, provider_config: &Value) -> Box<dyn SecretStore>;
}

fn registry() -> &'static HashMap<&'static str, Box<dyn SecretBackendFactory>> {
    static REG: OnceLock<HashMap<&'static str, Box<dyn SecretBackendFactory>>> = OnceLock::new();
    REG.get_or_init(|| {
        let mut map: HashMap<&'static str, Box<dyn SecretBackendFactory>> = HashMap::new();
        register_builtins(&mut map);
        map
    })
}

fn register_builtins(map: &mut HashMap<&'static str, Box<dyn SecretBackendFactory>>) {
    let factories: Vec<Box<dyn SecretBackendFactory>> = vec![
        Box::new(super::local::LocalFactory),
        Box::new(super::mock::MockFactory),
    ];
    for f in factories {
        map.insert(f.id(), f);
    }
}

pub fn factory(id: &str) -> Option<&'static dyn SecretBackendFactory> {
    registry().get(id).map(|b| b.as_ref())
}

pub fn list() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut entries: Vec<_> = registry()
        .values()
        .map(|f| (f.id(), f.label(), f.description()))
        .collect();
    entries.sort_by_key(|(_, label, _)| *label);
    entries
}

pub fn build_active(backend: &str, providers: &HashMap<String, Value>) -> Arc<dyn SecretStore> {
    let cfg = providers.get(backend).cloned().unwrap_or(Value::Null);
    let built = match factory(backend) {
        Some(f) => f.build(&cfg),
        None => {
            log::warn!(
                "[secrets] unknown backend '{}', falling back to local",
                backend
            );
            factory("local")
                .expect("local factory always registered")
                .build(&Value::Null)
        }
    };
    Arc::from(built)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_native_builtins() {
        // Native stores only: `local` (file, default) + `mock` (tests).
        // Bitwarden is a plugin now, not a built-in factory.
        let ids: Vec<_> = list().iter().map(|(id, _, _)| *id).collect();
        assert!(ids.contains(&"local"));
        assert!(ids.contains(&"mock"));
        assert!(!ids.contains(&"bitwarden"), "bitwarden is a plugin, not a factory");
    }

    #[test]
    fn factory_lookup_returns_some_for_builtins() {
        assert!(factory("local").is_some());
        assert!(factory("mock").is_some());
        assert!(factory("bitwarden").is_none(), "bitwarden is a plugin");
        assert!(factory("does-not-exist").is_none());
    }

    #[test]
    fn build_active_falls_back_to_local_for_unknown() {
        let s = build_active("unknown", &HashMap::new());
        assert_eq!(s.id(), "local");
    }
}
