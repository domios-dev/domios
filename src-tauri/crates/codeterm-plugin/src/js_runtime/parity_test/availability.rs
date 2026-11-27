//! Launch-binary availability parity. The verdict crosses the provider
//! boundary, so a JS drop-in and a native provider must read the same machine
//! the same way — including the unknown state, which is the one a drop-in
//! could most easily collapse into "not installed".

use super::*;

use crate::provider::ProviderAvailability;

const MISSING_BINARY: &str = "codeterm-parity-binary-that-cannot-exist";

struct NativeAvailabilityStub {
    binary_name: &'static str,
}

#[async_trait::async_trait]
impl AgentProvider for NativeAvailabilityStub {
    fn id(&self) -> &str {
        "availability-fixture"
    }

    fn display_name(&self) -> &str {
        "Availability Fixture"
    }

    fn binary_name(&self) -> &str {
        self.binary_name
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
}

fn js_with_binary_name(id: &str, binary_name: Option<&str>) -> JsProvider {
    use std::sync::Arc;

    let binary = binary_name
        .map(|name| format!(r#""binaryName":"{name}","#))
        .unwrap_or_default();
    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","hostIoJobs":true,"displayName":"Availability Fixture",{binary}
            "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                     "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}}}}}}"#
    );
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        id,
        "module.exports.default = {};".to_string(),
        Some(manifest_json),
    )
    .expect("availability fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}

#[test]
fn declared_launch_binary_agrees_across_the_provider_boundary() {
    let native = NativeAvailabilityStub {
        binary_name: MISSING_BINARY,
    };
    let js = js_with_binary_name("availability-missing", Some(MISSING_BINARY));

    assert_eq!(js.binary_name(), native.binary_name());
    assert_eq!(
        js.observe_launch_binary().availability,
        native.observe_launch_binary().availability
    );
    assert_ne!(
        native.observe_launch_binary().availability,
        ProviderAvailability::Available,
        "a binary that cannot exist must not read as installed"
    );
}

/// Omitted JS metadata takes the trait default on both sides of the boundary.
#[test]
fn default_launch_binary_agrees_across_the_provider_boundary() {
    let native = NativeAvailabilityStub {
        binary_name: "availability-default",
    };
    let js = js_with_binary_name("availability-default", None);

    assert_eq!(js.binary_name(), native.binary_name());
    assert_eq!(
        js.observe_launch_binary().availability,
        native.observe_launch_binary().availability
    );
}

/// `describe()` is I/O-free by contract on both paths, so neither may claim an
/// observation. Enumeration is the only thing allowed to fill the field in.
#[test]
fn describe_claims_no_observation_on_either_path() {
    let native = NativeAvailabilityStub {
        binary_name: MISSING_BINARY,
    };
    let js = js_with_binary_name("availability-describe", Some(MISSING_BINARY));

    assert_eq!(
        js.describe().availability,
        ProviderAvailability::Unknown,
        "a JS drop-in must not self-report installation"
    );
    assert_eq!(js.describe().availability, native.describe().availability);
}

#[test]
fn provider_cli_install_dirs_resolve_binary_outside_path_without_restart() {
    use std::sync::Arc;

    let dir = std::env::temp_dir().join(format!(
        "domios_install_dir_parity_{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let binary = format!("domios_declared_cli_{}", uuid::Uuid::new_v4().simple());
    #[cfg(windows)]
    let binary_path = dir.join(format!("{binary}.cmd"));
    #[cfg(not(windows))]
    let binary_path = dir.join(&binary);

    let platform = crate::provider::current_platform_tag();
    let dir_json = serde_json::to_string(&dir.to_string_lossy().to_string()).unwrap();
    let manifest_json = format!(
        r#"{{
            "id": "declared-dir-fixture",
            "version": "0.1.0",
            "hostIoJobs": true,
            "displayName": "Declared Dir Fixture",
            "binaryName": "{binary}",
            "cliBinary": "{binary}",
            "cliInstallDirs": {{ "{platform}": [{dir_json}] }},
            "capabilities": {{
                "agentProvider": true,
                "chatBackend": {{ "spawnable": true, "sessionConfig": true, "agentTasks": false }},
                "agentRuntime": {{
                    "runtime": "stdio-jsonrpc",
                    "command": {{ "binary": "{binary}", "args": [] }},
                    "requestMethods": {{ "initialize": ["initialize"] }},
                    "notificationMethods": []
                }}
            }},
            "permissions": {{ "subprocess": {{ "allow": ["{binary}"] }} }}
        }}"#
    );
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let observation = super::super::provider_observation::ProviderObservation::from_manifest(Some(
        &manifest_json,
    ));
    let sandbox = super::super::host_api::PluginSandbox::unrestricted()
        .with_extra_bin_dirs(manifest.cli_install_dirs(platform));
    let vm = super::super::PluginVm::from_source_with(
        "declared-dir-fixture",
        "module.exports.default = {};".to_string(),
        Some(manifest_json),
    )
    .expect("declared dir fixture vm");
    let js = JsProvider::new(manifest.clone(), Arc::new(vm));

    assert_eq!(
        js.observe_launch_binary().availability,
        ProviderAvailability::Unavailable
    );
    let before = crate::provider_cli::manifest_cli_status(&manifest, platform).unwrap();
    assert_eq!(before.installed, Some(false));
    assert!(before.searched.contains(&dir));

    assert_eq!(
        observation.version(&sandbox).status,
        crate::provider_version::ProbeStatus::Missing
    );
    let script = if cfg!(windows) {
        "@echo 1.0.0\r\n"
    } else {
        "#!/bin/sh\necho 1.0.0\n"
    };
    std::fs::write(&binary_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&binary_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&binary_path, perm).unwrap();
    }

    assert_eq!(
        js.observe_launch_binary().availability,
        ProviderAvailability::Available
    );
    assert_eq!(
        crate::provider_version::refresh(&binary_path)
            .version
            .as_deref(),
        Some("1.0.0")
    );
    assert_eq!(
        observation.version(&sandbox).version.as_deref(),
        Some("1.0.0")
    );
    let after = crate::provider_cli::manifest_cli_status(&manifest, platform).unwrap();
    assert_eq!(after.installed, Some(true));
    assert_eq!(after.resolved_path.as_deref(), Some(binary_path.as_path()));
    assert_eq!(
        js.resolved_cli_path().as_deref(),
        Some(binary_path.as_path())
    );
    assert_eq!(js.cli_install_dirs(platform), vec![dir.clone()]);
    assert_eq!(
        manifest
            .capabilities
            .agent_runtime
            .as_ref()
            .unwrap()
            .resolve_profile_executable_in(None, &manifest.cli_install_dirs(platform))
            .unwrap(),
        binary_path
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn desktop_cli_aliases_and_versioned_install_dirs_resolve_outside_path() {
    use std::sync::Arc;

    let root = std::env::temp_dir().join(format!(
        "domios_desktop_dir_parity_{}",
        uuid::Uuid::new_v4().simple()
    ));
    let version_dir = root.join("2.1.87");
    std::fs::create_dir_all(&version_dir).unwrap();
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let primary = format!("domios_primary_{suffix}");
    let alias = format!("domios_desktop_sidecar_{suffix}");
    #[cfg(windows)]
    let alias_path = version_dir.join(format!("{alias}.cmd"));
    #[cfg(not(windows))]
    let alias_path = version_dir.join(&alias);

    let platform = crate::provider::current_platform_tag();
    let wildcard_template = format!("{}/{}", root.to_string_lossy().replace('\\', "/"), "*");
    let dir_json = serde_json::to_string(&wildcard_template).unwrap();
    let manifest_json = format!(
        r#"{{
            "id": "desktop-alias-fixture",
            "version": "0.1.0",
            "hostIoJobs": true,
            "displayName": "Desktop Alias Fixture",
            "binaryName": "{primary}",
            "cliBinary": "{primary}",
            "cliBinaryAliases": ["{alias}"],
            "cliInstallDirs": {{ "{platform}": [{dir_json}] }},
            "capabilities": {{
                "agentProvider": true,
                "chatBackend": {{ "spawnable": true, "sessionConfig": true, "agentTasks": false }},
                "agentRuntime": {{
                    "runtime": "stdio-jsonrpc",
                    "command": {{ "binary": "{primary}", "args": [] }},
                    "requestMethods": {{ "initialize": ["initialize"] }},
                    "notificationMethods": []
                }}
            }},
            "permissions": {{ "subprocess": {{ "allow": ["{primary}", "{alias}"] }} }}
        }}"#
    );
    let manifest: super::super::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "desktop-alias-fixture",
        "module.exports.default = {};".to_string(),
        Some(manifest_json),
    )
    .expect("desktop alias fixture vm");
    let js = JsProvider::new(manifest.clone(), Arc::new(vm));

    assert_eq!(
        js.observe_launch_binary().availability,
        ProviderAvailability::Unavailable
    );

    let script = if cfg!(windows) {
        "@echo 2.1.87\r\n"
    } else {
        "#!/bin/sh\necho 2.1.87\n"
    };
    std::fs::write(&alias_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&alias_path).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&alias_path, perm).unwrap();
    }

    assert_eq!(
        js.observe_launch_binary().availability,
        ProviderAvailability::Available
    );
    let status = crate::provider_cli::manifest_cli_status(&manifest, platform).unwrap();
    assert_eq!(status.installed, Some(true));
    assert_eq!(status.resolved_path.as_deref(), Some(alias_path.as_path()));
    assert_eq!(
        js.resolved_cli_path().as_deref(),
        Some(alias_path.as_path())
    );
    assert_eq!(
        manifest
            .capabilities
            .agent_runtime
            .as_ref()
            .unwrap()
            .resolve_profile_executable_with_aliases_in(
                None,
                &manifest.cli_binary_aliases,
                &manifest.cli_install_dirs(platform),
            )
            .unwrap(),
        alias_path
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bundled_providers_with_installers_declare_install_locations() {
    let plugins_dir = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut checked = 0usize;
    for entry in std::fs::read_dir(&plugins_dir).expect("plugins dir") {
        let manifest_path = entry.unwrap().path().join("plugin.json");
        if !manifest_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&manifest_path).unwrap();
        let manifest: super::super::PluginManifest = serde_json::from_str(&text).unwrap();
        if !manifest.capabilities.agent_provider || manifest.installer.is_empty() {
            continue;
        }
        checked += 1;
        assert!(
            !manifest.cli_install_dirs.is_empty(),
            "bundled provider {} declares installers without cliInstallDirs",
            manifest.id
        );
        for (platform, dirs) in &manifest.cli_install_dirs {
            assert!(
                !dirs.is_empty(),
                "bundled provider {} has empty cliInstallDirs for {platform}",
                manifest.id
            );
        }
    }
    assert!(
        checked > 0,
        "expected at least one bundled installable provider"
    );
}
