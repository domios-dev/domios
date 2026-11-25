use super::*;

#[test]
fn inference_prefers_specific_hardware_to_portable_and_headless_hints() {
    let mut facts = MachineFacts {
        os: "linux",
        virtualized: true,
        small_device: true,
        portable: true,
        headless: true,
    };
    assert_eq!(facts.kind(), MachineKind::Vm);
    facts.virtualized = false;
    assert_eq!(facts.kind(), MachineKind::Small);
    facts.small_device = false;
    assert_eq!(facts.kind(), MachineKind::Laptop);
    facts.portable = false;
    assert_eq!(facts.kind(), MachineKind::Server);
    facts.headless = false;
    assert_eq!(facts.kind(), MachineKind::Desktop);
    facts.os = "macos";
    assert_eq!(facts.kind(), MachineKind::Mac);
    facts.os = "android";
    assert_eq!(facts.kind(), MachineKind::Phone);
    facts.os = "";
    assert_eq!(facts.kind(), MachineKind::Other);
}

#[test]
fn saved_choice_is_scoped_to_the_installation_identity() {
    let dir = std::env::temp_dir().join(format!("mesh-machine-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("machine-identity.json");
    let choice = Revision {
        identity: MachineIdentity {
            kind: MachineKind::Laptop,
        },
        number: 2,
    };
    save(&path, "this-installation", choice).unwrap();
    assert_eq!(load(&path, "this-installation"), Some(choice));
    assert_eq!(load(&path, "another-installation"), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn load_drops_legacy_color_field_while_rejecting_unknown_fields() {
    let dir = std::env::temp_dir().join(format!("mesh-machine-legacy-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("machine-identity.json");
    std::fs::write(
        &path,
        r#"{"instance_id":"this-installation","revision":{"identity":{"kind":"laptop","color":"teal"},"number":3}}"#,
    )
    .unwrap();
    assert_eq!(
        load(&path, "this-installation"),
        Some(Revision {
            identity: MachineIdentity {
                kind: MachineKind::Laptop,
            },
            number: 3,
        })
    );
    std::fs::write(
        &path,
        r#"{"instance_id":"this-installation","revision":{"identity":{"kind":"laptop","extra":true},"number":4}}"#,
    )
    .unwrap();
    assert_eq!(load(&path, "this-installation"), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn mixed_version_peer_payloads_deserialize_and_serialize_safely() {
    let legacy: MachineIdentity =
        serde_json::from_str(r#"{"kind":"server","color":"violet"}"#).unwrap();
    assert_eq!(
        legacy,
        MachineIdentity {
            kind: MachineKind::Server,
        }
    );

    let kind_only: MachineIdentity = serde_json::from_str(r#"{"kind":"mac"}"#).unwrap();
    assert_eq!(
        kind_only,
        MachineIdentity {
            kind: MachineKind::Mac,
        }
    );

    assert!(serde_json::from_str::<MachineIdentity>(r#"{"kind":"mac","extra":true}"#).is_err());

    let encoded = serde_json::to_value(MachineIdentity {
        kind: MachineKind::Laptop,
    })
    .unwrap();
    assert_eq!(
        encoded,
        serde_json::json!({"kind": "laptop", "color": LEGACY_WIRE_COLOR})
    );

    let peers: crate::types::MeshPeersResponse = serde_json::from_value(serde_json::json!({
        "local_machine_identity": { "kind": "desktop", "color": "blue" },
        "machine_identities": {
            "peer-1232": { "kind": "laptop", "color": "teal" },
            "peer-1233": { "kind": "server" }
        },
        "installations": [],
        "paired": [],
        "local_instance_name": "Desk"
    }))
    .unwrap();
    assert_eq!(
        peers.local_machine_identity,
        MachineIdentity {
            kind: MachineKind::Desktop,
        }
    );
    assert_eq!(
        peers.machine_identities.get("peer-1232"),
        Some(&MachineIdentity {
            kind: MachineKind::Laptop,
        })
    );
    assert_eq!(
        peers.machine_identities.get("peer-1233"),
        Some(&MachineIdentity {
            kind: MachineKind::Server,
        })
    );
}
