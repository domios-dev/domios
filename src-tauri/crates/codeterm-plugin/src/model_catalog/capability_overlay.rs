use super::ModelInfo;
use parking_lot::RwLock;
use std::{collections::BTreeMap, sync::OnceLock};
type Overlays = BTreeMap<(String, String), Vec<ModelInfo>>;
fn overlays() -> &'static RwLock<Overlays> {
    static OVERLAYS: OnceLock<RwLock<Overlays>> = OnceLock::new();
    OVERLAYS.get_or_init(RwLock::default)
}
pub fn put(provider: &str, project: &str, models: Vec<ModelInfo>) {
    overlays()
        .write()
        .insert((provider.into(), project.into()), models);
    super::invalidate(provider);
}
pub fn clear(provider: &str) {
    let changed = {
        let mut overlays = overlays().write();
        let before = overlays.len();
        overlays.retain(|(id, _), _| id != provider);
        before != overlays.len()
    };
    if changed {
        super::invalidate(provider);
    }
}
pub(super) fn present(provider: &str) -> bool {
    overlays().read().keys().any(|(id, _)| id == provider)
}
pub(super) fn merge(provider: &str, mut models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    for ((id, _), entries) in overlays().read().iter() {
        if id != provider {
            continue;
        }
        for entry in entries {
            if let Some(existing) = models.iter_mut().find(|model| model.id == entry.id) {
                *existing = entry.clone();
            } else {
                models.push(entry.clone());
            }
        }
    }
    models
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn claude_init_probe_overlay_survives_other_live_refresh_and_clears() {
        let id = format!("init-overlay-{}", uuid::Uuid::new_v4());
        let seed = ModelInfo {
            id: "seed".into(),
            display_name: "Seed".into(),
            ..Default::default()
        };
        super::super::register(
            &id,
            super::super::RegisteredDefaults {
                models: vec![seed.clone()],
                default: Some("seed".into()),
            },
        );
        let live = ModelInfo {
            id: "live-alias".into(),
            display_name: "Live".into(),
            ..Default::default()
        };
        put(&id, "project-a", vec![live]);
        assert!(super::super::resolve_models(&id)
            .iter()
            .any(|model| model.id == "live-alias"));
        super::super::registry().write().insert(
            id.clone(),
            super::super::ProviderModels::live(
                vec![seed],
                Some("seed".into()),
                std::time::SystemTime::now(),
            ),
        );
        assert!(super::super::resolve_live_models(&id)
            .unwrap()
            .iter()
            .any(|model| model.id == "live-alias"));
        clear(&id);
        assert!(!super::super::resolve_models(&id)
            .iter()
            .any(|model| model.id == "live-alias"));
        super::super::unregister(&id);
    }
}
