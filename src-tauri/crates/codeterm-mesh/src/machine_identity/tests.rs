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
    assert_eq!(
        MachineIdentity::inferred("same-id", "linux", false).color,
        MachineIdentity::inferred("same-id", "windows", false).color
    );
}

#[test]
fn saved_choice_is_scoped_to_the_installation_identity() {
    let dir = std::env::temp_dir().join(format!("mesh-machine-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("machine-identity.json");
    let choice = Revision {
        identity: MachineIdentity {
            kind: MachineKind::Laptop,
            color: MachineColor::Teal,
        },
        number: 2,
    };
    save(&path, "this-installation", choice).unwrap();
    assert_eq!(load(&path, "this-installation"), Some(choice));
    assert_eq!(load(&path, "another-installation"), None);
    std::fs::remove_dir_all(&dir).unwrap();
}
