use std::path::{Path, PathBuf};

use codeterm_plugin::model_catalog;
use codeterm_plugin::provider::ModelInfo;

fn user_root() -> PathBuf {
    dirs::home_dir()
        .expect("home directory")
        .join(super::dir_name())
}

fn assert_isolated(path: &Path) {
    assert!(path.starts_with(std::env::temp_dir()), "{}", path.display());
    assert!(!path.starts_with(user_root()), "{}", path.display());
}

#[test]
fn data_root_writers_resolve_under_the_isolated_temp_root() {
    let identity = crate::mesh::peers::mesh_dir().join("identity.json");
    assert!(!super::codeterm_instance_id().is_empty());
    assert!(identity.is_file());

    let id = format!("data-root-guard-{}", uuid::Uuid::new_v4());
    model_catalog::put_live(
        &id,
        vec![ModelInfo {
            id: "guard".into(),
            ..Default::default()
        }],
        None,
    );
    let cache = model_catalog::models_dir().join(format!("{id}.cache.json"));
    assert!(cache.is_file());
    model_catalog::unregister(&id);
    let _ = std::fs::remove_file(&cache);

    for path in [
        super::data_dir_path(),
        identity,
        cache,
        crate::service::general_agent::general_dir(),
        crate::service::general_agent::secrets_dir(),
        crate::service::bridge_threads::registry_path(),
    ] {
        assert_isolated(&path);
    }
}
