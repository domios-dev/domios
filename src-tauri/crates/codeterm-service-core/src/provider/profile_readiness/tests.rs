use super::*;
use codeterm_plugin::agent_runtime::AgentRuntimeCommand;
use codeterm_plugin::provider::ProviderAvailability;
use serde_json::json;

#[test]
fn provider_readiness_uses_receipts_and_rechecks_changed_profiles() {
    let root = tempfile::tempdir().unwrap();
    let mut capability: AgentRuntimeCapability = serde_json::from_value(json!({
        "runtime":"stdio-jsonrpc",
        "command":{"binary":"provider-cli-that-does-not-exist","args":[]},
        "requestMethods":{}, "notificationMethods":[],
        "launchProfiles":{"configured":{"binary":"fixture-agent","args":[],"ownerConfigured":true}}
    }))
    .unwrap();
    let observe = |capability: &AgentRuntimeCapability| {
        observe_from_root(
            "fixture",
            capability,
            Ok(root.path().to_path_buf()),
            &parking_lot::Mutex::new(ProfileCache::default()),
        )
        .unwrap()
    };
    assert_eq!(observe(&capability).installed, Some(false));
    let profile = &capability.launch_profiles.as_ref().unwrap()["configured"];
    launch_profile_install::confirm_local(
        root.path(),
        "configured",
        profile,
        AgentRuntimeCommand {
            binary: std::env::current_exe().unwrap().to_str().unwrap().into(),
            args: vec![],
        },
        true,
    )
    .unwrap();
    let ready = observe(&capability);
    assert_eq!(ready.installed, Some(true));
    assert_eq!(ready.availability, ProviderAvailability::Available);
    assert!(!ready.required);
    assert!(ready.binary.is_none());
    assert_eq!(ready.launch_profiles.unwrap()[0].id, "configured");
    capability
        .launch_profiles
        .as_mut()
        .unwrap()
        .get_mut("configured")
        .unwrap()
        .args
        .push("changed".into());
    assert_eq!(observe(&capability).installed, Some(false));
    std::fs::write(root.path().join("configured.json"), b"malformed receipt").unwrap();
    assert_eq!(observe(&capability).installed, None);
    assert_eq!(
        observe(&capability).availability,
        ProviderAvailability::Unknown
    );
    std::fs::remove_file(root.path().join("configured.json")).unwrap();
    assert_eq!(observe(&capability).installed, Some(false));
    capability.launch_profiles = None;
    assert!(observe_from_root(
        "fixture",
        &capability,
        Ok(root.path().to_path_buf()),
        &parking_lot::Mutex::new(ProfileCache::default()),
    )
    .is_none());
}

#[test]
fn profile_readiness_cache_reuses_observations_until_revision_or_declaration_changes() {
    use std::cell::Cell;
    let cache = parking_lot::Mutex::new(ProfileCache::default());
    let profile: codeterm_plugin::agent_runtime::RuntimeLaunchProfile =
        serde_json::from_value(json!({"binary":"fixture", "args":[]})).unwrap();
    let reads = Cell::new(0);
    let observe = || {
        reads.set(reads.get() + 1);
        Some(true)
    };
    assert_eq!(
        cached_profile(&cache, "provider", "first", &profile, 1, observe),
        Some(true)
    );
    assert_eq!(
        cached_profile(&cache, "provider", "first", &profile, 1, || panic!(
            "cached status performed I/O"
        )),
        Some(true)
    );
    assert_eq!(reads.get(), 1);
    assert_eq!(
        cached_profile(&cache, "provider", "second", &profile, 1, observe),
        Some(true)
    );
    assert_eq!(reads.get(), 2);
    assert_eq!(
        cached_profile(&cache, "provider", "first", &profile, 2, || Some(false)),
        Some(false)
    );
    let mut changed = profile.clone();
    changed.args.push("changed".into());
    assert_eq!(
        cached_profile(&cache, "provider", "first", &changed, 2, || None),
        None
    );
    assert_eq!(
        cached_profile(&cache, "provider", "first", &changed, 2, || panic!(
            "unknown status must be cached"
        )),
        None
    );
}

#[test]
fn readiness_invalidation_during_observation_cannot_publish_a_stale_cache_hit() {
    use std::cell::Cell;
    let generation = Cell::new(1);
    let cache = parking_lot::Mutex::new(ProfileCache::default());
    let profile: codeterm_plugin::agent_runtime::RuntimeLaunchProfile =
        serde_json::from_value(json!({"binary":"fixture", "args":[]})).unwrap();
    assert_eq!(
        cached_profile(
            &cache,
            "provider",
            "profile",
            &profile,
            generation.get(),
            || {
                generation.set(2);
                Some(false)
            }
        ),
        Some(false)
    );
    assert_eq!(
        cached_profile(
            &cache,
            "provider",
            "profile",
            &profile,
            generation.get(),
            || Some(true)
        ),
        Some(true)
    );
    assert_eq!(
        cached_profile(
            &cache,
            "provider",
            "profile",
            &profile,
            generation.get(),
            || panic!("fresh observation must be retained")
        ),
        Some(true)
    );
}
