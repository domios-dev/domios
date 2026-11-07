use super::*;
use serde_json::json;

#[test]
fn owner_profile_add_remove_round_trip_preserves_references_and_arguments() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profiles.json");
    let profile: RuntimeLaunchProfile = serde_json::from_value(json!({"binary":"fixture-agent","args":["--stdio"],"env":{"MODE":"fixture"},"secretEnv":{"TOKEN":"domios:provider:acp:fixture"},"ownerConfigured":true})).unwrap();
    update_at(&path, |profiles| {
        profiles.insert("fixture".into(), profile.clone());
        Ok(())
    })
    .unwrap();
    assert_eq!(read(&path).unwrap()["fixture"], profile);
    update_at(&path, |profiles| {
        assert!(profiles.remove("fixture").is_some());
        Ok(())
    })
    .unwrap();
    assert!(read(&path).unwrap().is_empty());
}

#[test]
fn invalid_or_oversized_storage_fails_closed_and_names_cannot_escape() {
    for name in ["", "two words", "../profile", "NUL", "CON", "profile.json"] {
        assert!(valid_name(name).is_err(), "{name}");
    }
    assert!(valid_name("fixture-agent_2").is_ok());
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profiles.json");
    std::fs::write(&path, b"{broken}").unwrap();
    assert!(read(&path).is_err());
    std::fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
    assert!(read(&path).is_err());
}

#[test]
fn cached_profile_icon_refresh_matches_only_its_declared_url() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("profiles.json");
    let declarations = json!({
        "first": {"binary":"first-agent","args":[],"ownerConfigured":true,"iconUrl":"https://example.org/first.svg"},
        "second": {"binary":"second-agent","args":[],"ownerConfigured":true,"iconUrl":"https://example.org/second.svg"}
    });
    let bytes = serde_json::to_vec(&declarations).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    cache().lock().insert(
        path.clone(),
        Ok(serde_json::from_value(declarations).unwrap()),
    );
    let icon = "<svg><path d=\"M1 1\"/></svg>";
    assert!(refresh_icon_at(&path, "https://example.org/first.svg", icon).unwrap());
    assert!(!refresh_icon_at(&path, "https://example.org/first.svg", icon).unwrap());
    assert!(!refresh_icon_at(&path, "https://example.org/unknown.svg", icon).unwrap());
    let projected = cache().lock().get(&path).unwrap().clone().unwrap();
    assert_eq!(projected["first"].icon_svg.as_deref(), Some(icon));
    assert!(projected["second"].icon_svg.is_none());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    cache().lock().remove(&path);
}

#[test]
fn derived_owner_profiles_show_the_sanitized_bundle_icon_their_base_declares() {
    let mut capability: AgentRuntimeCapability = serde_json::from_value(json!({
        "runtime":"stdio-jsonrpc","command":{"binary":"fixture-agent","args":[]},
        "requestMethods":{"initialize":["initialize"]},"notificationMethods":[],
        "launchProfiles":{"base":{"binary":"base-agent","args":[],"icon":"base-color.svg"}}
    }))
    .unwrap();
    let bundled = "<svg><path d=\"M1 1\"/></svg>";
    capability
        .launch_profiles
        .as_mut()
        .unwrap()
        .get_mut("base")
        .unwrap()
        .icon_svg = Some(bundled.into());
    let owner: Profiles = serde_json::from_value(json!({
        "derived":{"binary":"derived-agent","args":[],"ownerConfigured":true,"icon":"base-color.svg"},
        "undeclared":{"binary":"other-agent","args":[],"ownerConfigured":true,"icon":"absent.svg"}
    }))
    .unwrap();
    let profiles = overlay_with(capability.clone(), owner)
        .unwrap()
        .launch_profiles
        .unwrap();
    assert_eq!(profiles["derived"].icon_svg.as_deref(), Some(bundled));
    assert!(profiles["undeclared"].icon_svg.is_none());
    let colliding: Profiles = serde_json::from_value(json!({
        "base":{"binary":"derived-agent","args":[],"ownerConfigured":true}
    }))
    .unwrap();
    assert!(overlay_with(capability, colliding).is_err());
}

#[test]
fn installed_registry_profiles_gain_the_plugin_registry_default_on_load() {
    let capability: AgentRuntimeCapability = serde_json::from_value(json!({
        "runtime":"stdio-jsonrpc","command":{"binary":"fixture-agent","args":[]},
        "requestMethods":{"initialize":["initialize"]},"notificationMethods":[],
        "launchProfiles":{"base":{"binary":"base-agent","args":[]}},
        "registryDefaults":{"fixture-acp":{"limitsSource":{"kind":"account","provider":"fixture-native"}}}
    }))
    .unwrap();
    let stored: Profiles = serde_json::from_value(json!({
        "installed":{"binary":"installed-agent","args":[],"ownerConfigured":true,"registryId":"fixture-acp"},
        "chosen":{"binary":"chosen-agent","args":[],"ownerConfigured":true,"registryId":"fixture-acp",
            "limitsSource":{"kind":"unavailable","reason":"Owner reason"}},
        "foreign":{"binary":"foreign-agent","args":[],"ownerConfigured":true,"registryId":"unknown-acp"}
    }))
    .unwrap();
    let profiles = overlay_with(capability, stored.clone())
        .unwrap()
        .launch_profiles
        .unwrap();
    assert_eq!(
        profiles["installed"].limits_source,
        Some(crate::agent_runtime::RuntimeProfileLimitsSource::Account {
            provider: "fixture-native".into()
        })
    );
    assert_eq!(
        profiles["chosen"].limits_source,
        stored["chosen"].limits_source
    );
    assert_eq!(profiles["foreign"].limits_source, None);
    assert_eq!(profiles["base"].limits_source, None);
    assert_eq!(
        stored["installed"].limits_source, None,
        "the stored record is not rewritten"
    );
}
