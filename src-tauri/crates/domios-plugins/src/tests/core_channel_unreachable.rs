use super::*;
use std::path::Path;

const PRIVATE_SOURCE: &str = "https://github.com/rollacode/codeterm.git#master:plugins";

struct Fixture {
    root: PathBuf,
    id: &'static str,
    bundled_version: String,
    previous: Vec<(&'static str, Option<String>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new() -> Self {
        let lock = crate::test_env_lock()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = std::env::temp_dir().join(format!(
            "codeterm-core-unreachable-{}",
            uuid::Uuid::new_v4()
        ));
        let previous = [
            "CODETERM_DATA_DIR",
            "CODETERM_SYSTEM_PLUGIN_CHANNEL_URL",
            "CODETERM_TEST_GIT_BIN",
        ]
        .map(|key| (key, std::env::var(key).ok()))
        .to_vec();
        std::env::set_var("CODETERM_DATA_DIR", &root);
        std::env::set_var("CODETERM_SYSTEM_PLUGIN_CHANNEL_URL", PRIVATE_SOURCE);
        std::env::set_var("CODETERM_TEST_GIT_BIN", denying_git(&root));
        let id = SYSTEM_PLUGIN_IDS[0];
        let (directory, manifest) = bundled_metadata(id).expect("bundled core plugin");
        let bundled_version = manifest.version.expect("bundled version");
        install_bundled(id, &bundled_version);
        register_stale_core_cache(&root, id, directory);
        Self {
            root,
            id,
            bundled_version,
            previous,
            _lock: lock,
        }
    }

    fn ctx(&self) -> PluginCtx {
        PluginCtx {
            data_root: self.root.clone(),
            version: Some("99.0.0".into()),
        }
    }

    fn installed_version(&self) -> Option<String> {
        manifest_version(&plugin_dir(&PluginId::validated(self.id)))
            .ok()
            .flatten()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn denying_git(root: &Path) -> PathBuf {
    std::fs::create_dir_all(root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = root.join("fake-git.sh");
        std::fs::write(
            &path,
            "#!/bin/sh\necho 'remote: Repository not found.' >&2\necho \"fatal: repository 'https://github.com/rollacode/codeterm.git/' not found\" >&2\nexit 128\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
    #[cfg(windows)]
    {
        let path = root.join("fake-git.cmd");
        std::fs::write(
            &path,
            "@echo off\r\necho remote: Repository not found. 1>&2\r\necho fatal: repository 'https://github.com/rollacode/codeterm.git/' not found 1>&2\r\nexit /b 128\r\n",
        )
        .unwrap();
        path
    }
}

fn install_bundled(id: &str, version: &str) {
    let dir = plugin_dir(&PluginId::validated(id));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::json!({"id": id, "version": version}).to_string(),
    )
    .unwrap();
}

fn register_stale_core_cache(root: &Path, id: &str, directory: &str) {
    let cache = root.join("channels").join(SYSTEM_CHANNEL_NAME);
    std::fs::create_dir_all(cache.join(directory)).unwrap();
    std::fs::write(
        cache.join("channel.json"),
        serde_json::json!({
            "name": SYSTEM_CHANNEL_NAME,
            "plugins": [{"id": id, "version": "0.0.1", "path": directory}],
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        cache.join(directory).join("plugin.json"),
        serde_json::json!({"id": id, "version": "0.0.1"}).to_string(),
    )
    .unwrap();
    let mut registry = ChannelRegistry::default();
    registry.channels.insert(
        SYSTEM_CHANNEL_NAME.into(),
        ChannelRecord {
            name: SYSTEM_CHANNEL_NAME.into(),
            url: PRIVATE_SOURCE.into(),
            path: cache.display().to_string(),
            kind: ChannelKind::Git,
        },
    );
    write_channel_registry(&registry).unwrap();
}

#[test]
fn unreachable_core_source_is_explained_and_leaves_the_bundled_set_silent() {
    let fixture = Fixture::new();

    let inventory = update_inventory(fixture.ctx(), true).expect("inventory still answers");

    let error = inventory["errors"][SYSTEM_CHANNEL_NAME]
        .as_str()
        .expect("core channel failure is reported");
    assert!(
        error.starts_with("Plugin channel unavailable: repository not found"),
        "{error}"
    );
    assert!(
        error.contains("github.com/rollacode/codeterm.git"),
        "{error}"
    );
    let item = inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == fixture.id)
        .expect("bundled core plugin listed");
    assert_eq!(item["state"], "core_silent");
    assert_eq!(item["reason"], "same_version");
    assert_eq!(item["latest"], fixture.bundled_version.as_str());
    assert_eq!(
        fixture.installed_version().as_deref(),
        Some(fixture.bundled_version.as_str())
    );
}

#[test]
fn channel_refresh_names_the_concrete_cause() {
    let fixture = Fixture::new();

    let error = run(
        fixture.ctx(),
        PluginAction::Channel(PluginChannelAction::Refresh {
            name: Some(SYSTEM_CHANNEL_NAME.into()),
        }),
    )
    .unwrap_err();

    assert!(error.contains("repository not found"), "{error}");
    assert!(!error.contains("Plugin updates are unavailable right now"));
}

#[test]
fn stale_core_cache_never_downgrades_or_reports_local_ahead() {
    let fixture = Fixture::new();
    let _guard = enter_ctx(fixture.ctx());

    let result = update_system_cached(None).expect("an older cache is not an error");

    assert_eq!(result["updated"], serde_json::json!([]));
    assert_eq!(result["warnings"], serde_json::json!([]));
    assert_eq!(result["retired"], serde_json::json!([]));
    assert_eq!(
        fixture.installed_version().as_deref(),
        Some(fixture.bundled_version.as_str())
    );
    let status = installed_update_status(fixture.id, fixture.installed_version().as_deref(), None);
    assert_eq!(status["available"], false);
    assert_eq!(status["basis"], "bundled_manifest");
}
