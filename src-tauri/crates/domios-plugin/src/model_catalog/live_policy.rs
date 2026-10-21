use super::*;
use crate::provider::ModelCatalogueCapability;
use std::collections::HashSet;

fn required_catalogues() -> &'static RwLock<HashSet<String>> {
    static REQUIRED: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();
    REQUIRED.get_or_init(|| RwLock::new(HashSet::new()))
}

pub fn register_catalogue_capability(provider_id: &str, capability: &ModelCatalogueCapability) {
    let mut catalogues = required_catalogues().write();
    if capability.live_discovery_required {
        catalogues.insert(provider_id.to_string());
    } else {
        catalogues.remove(provider_id);
    }
}

pub(super) fn required(provider_id: &str) -> bool {
    required_catalogues().read().contains(provider_id)
}

pub(super) fn clear(provider_id: &str) {
    required_catalogues().write().remove(provider_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_live_catalogue_excludes_seed_and_override_only_models() {
        let id = "required-live-fixture-564";
        register_catalogue_capability(id, &ModelCatalogueCapability::authoritative_live());
        register(
            id,
            RegisteredDefaults {
                models: vec![ModelInfo {
                    id: "router/dead".into(),
                    display_name: "Dead".into(),
                    ..Default::default()
                }],
                default: Some("router/dead".into()),
            },
        );
        assert!(resolve_models(id).is_empty());
        assert!(resolve_default(id).is_none());
        std::fs::write(
            override_path(id),
            "[[models]]\nid = \"router/dead\"\ndisplay_name = \"Dead override\"\n",
        )
        .unwrap();
        put_live(
            id,
            vec![ModelInfo {
                id: "router/working".into(),
                display_name: "Working".into(),
                ..Default::default()
            }],
            Some("router/working".into()),
        );
        let models = resolve_models(id);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "router/working");
        assert_eq!(models[0].display_name, "Working");
        assert_eq!(resolve_default(id).as_deref(), Some("router/working"));
        assert!(resolve_catalog_model(id, "router/dead").is_none());
        unregister(id);
        let _ = std::fs::remove_file(override_path(id));
        let _ = std::fs::remove_file(models_dir().join(format!("{id}.cache.json")));
    }
}
