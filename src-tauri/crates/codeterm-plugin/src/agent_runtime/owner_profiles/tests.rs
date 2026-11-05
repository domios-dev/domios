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
