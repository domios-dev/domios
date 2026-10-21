use std::path::{Component, Path, PathBuf};

/// The user-owned settings file inside a plugin's directory.
pub const CONFIG_FILE: &str = "config.yaml";

/// The one `config.yaml` both `plugin config` and the host use for a plugin id. An installed
/// copy under the user plugins root wins; a bundle loaded from elsewhere keeps its own file.
pub fn config_yaml_path(user_plugins_root: &Path, plugin_id: &str, bundle_dir: &Path) -> PathBuf {
    let mut components = Path::new(plugin_id).components();
    let single_name = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    );
    let installed = user_plugins_root.join(plugin_id);
    if single_name && installed.join("plugin.json").is_file() {
        installed.join(CONFIG_FILE)
    } else {
        bundle_dir.join(CONFIG_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_copy_owns_the_config_of_a_bundle_loaded_elsewhere() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("plugins");
        let workspace_bundle = root.path().join("repo/plugins/provider-codex");
        std::fs::create_dir_all(user.join("codex")).unwrap();
        std::fs::write(user.join("codex/plugin.json"), "{}").unwrap();

        assert_eq!(
            config_yaml_path(&user, "codex", &workspace_bundle),
            user.join("codex").join(CONFIG_FILE)
        );
        assert_eq!(
            config_yaml_path(&user, "codex", &user.join("codex")),
            user.join("codex").join(CONFIG_FILE)
        );
    }

    #[test]
    fn bundle_keeps_its_config_without_an_installed_copy() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("plugins");
        let bundle = root.path().join("repo/plugins/git");
        std::fs::create_dir_all(user.join("git")).unwrap();

        assert_eq!(
            config_yaml_path(&user, "git", &bundle),
            bundle.join(CONFIG_FILE)
        );
        assert_eq!(
            config_yaml_path(&user, "absent", &bundle),
            bundle.join(CONFIG_FILE)
        );
    }

    #[test]
    fn non_name_ids_never_escape_the_bundle() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("plugins");
        let bundle = root.path().join("bundle");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(root.path().join("plugin.json"), "{}").unwrap();

        for id in ["..", "../x", "a/b", ""] {
            assert_eq!(
                config_yaml_path(&user, id, &bundle),
                bundle.join(CONFIG_FILE),
                "{id}"
            );
        }
    }
}
