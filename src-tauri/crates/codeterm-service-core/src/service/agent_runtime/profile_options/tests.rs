use super::*;
use serde_json::json;

#[test]
fn last_declared_options_round_trip_and_bind_to_profile() {
    let root = tempfile::tempdir().unwrap();
    let profile: RuntimeLaunchProfile =
        serde_json::from_value(json!({"binary":"fixture-agent","args":[]})).unwrap();
    let settings: RuntimeSettings = serde_json::from_value(json!({"configOptions":[
        {"id":"model-choice","name":"Model","category":"model","currentValue":"M","options":[{"value":"M","name":"Model M"}]},
        {"id":"effort-choice","name":"Effort","category":"thought_level","currentValue":"E","options":[{"value":"E","name":"Effort E"}]},
        {"id":"other","name":"Other","currentValue":"x","options":[]}
    ]})).unwrap();
    let saved = snapshot(&profile, &settings).unwrap();
    write(&path(root.path(), "fixture").unwrap(), &saved).unwrap();
    let observed = read(root.path(), "fixture", &profile);
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].id, "model-choice");
    assert_eq!(observed[1].options[0].name, "Effort E");
    let changed = RuntimeLaunchProfile {
        binary: "another-agent".into(),
        ..profile.clone()
    };
    assert!(read(root.path(), "fixture", &changed).is_empty());
    let cleared = snapshot(
        &profile,
        &RuntimeSettings {
            config_options: Some(Vec::new()),
            ..Default::default()
        },
    )
    .unwrap();
    write(&path(root.path(), "fixture").unwrap(), &cleared).unwrap();
    assert!(read(root.path(), "fixture", &profile).is_empty());
    assert!(snapshot(&profile, &RuntimeSettings::default()).is_none());
    assert!(path(root.path(), "../escape").is_none());
    assert!(path(root.path(), "NUL").is_none());
}
