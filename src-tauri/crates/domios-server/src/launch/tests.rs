use super::*;
use codeterm_service_core::ResolvedInstancePaths;
use codeterm_types::server_bootstrap::ServerIdentity;

#[test]
fn process_split_contract_probe_alias_requires_both_roots_and_owned_sink() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("fresh");
    let normal = ResolvedInstancePaths::resolve(&data, &data.join("config.toml")).unwrap();
    let mut options = LaunchOptions {
        instance: InstanceSelector::Development {
            instance: Some("split-oracle-fixture".into()),
            port: 18886,
        },
        resources: root.path().into(),
        probe_sink: Some(data.join("process-init.jsonl")),
        probe_identity_alias: Some(ServerIdentity {
            data: data.join("absent/.."),
            config: data.join("config.toml"),
        }),
    };
    assert_eq!(verify_identity(&options, normal.clone()).unwrap(), normal);
    options.probe_identity_alias.as_mut().unwrap().config = root.path().join("other-config");
    assert!(verify_identity(&options, normal.clone()).is_err());
    options.probe_identity_alias.as_mut().unwrap().config = data.join("config.toml");
    options.probe_sink = Some(root.path().join("foreign-sink"));
    assert!(verify_identity(&options, normal.clone()).is_err());
    options.probe_sink = Some(data.join("process-init.jsonl"));
    options.instance = InstanceSelector::Production;
    assert!(options.validate(false).is_err());
    assert!(verify_identity(&options, normal).is_err());
    assert!(!data.exists());
}

#[test]
fn process_split_contract_ordinary_bootstrap_omits_probe_alias() {
    let options: LaunchOptions = serde_json::from_value(serde_json::json!({
        "instance": { "kind": "development", "instance": "fixture", "port": 18886 },
        "resources": "resources", "probe_sink": null
    }))
    .unwrap();
    assert!(options.probe_identity_alias.is_none());
    assert!(serde_json::to_value(options)
        .unwrap()
        .get("probe_identity_alias")
        .is_none());
}
