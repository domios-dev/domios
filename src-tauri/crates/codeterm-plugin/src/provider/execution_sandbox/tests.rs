use super::*;

fn capability() -> ExecutionSandboxCapability {
    ExecutionSandboxCapability {
        platforms: vec!["fixture-os".into()],
        read_only: Some(ExecutionSandboxMode {
            launch_args: vec!["opaque-read".into()],
            runtime_mode: Some("opaque-read-mode".into()),
        }),
        mutating: Some(ExecutionSandboxMode {
            launch_args: vec!["opaque-write".into()],
            runtime_mode: Some("opaque-write-mode".into()),
        }),
        forbidden_argument_prefixes: vec!["--escape".into(), "-c".into()],
        ..Default::default()
    }
}

#[test]
fn intent_mapping_uses_declared_modes_and_transport_coverage() {
    let cap = capability();
    for runtime in [false, true] {
        assert_eq!(
            cap.resolve(&ExecutionIntent::ReadOnly, "fixture-os", runtime)
                .unwrap(),
            cap.read_only
        );
        assert_eq!(
            cap.resolve(&ExecutionIntent::Mutating, "fixture-os", runtime)
                .unwrap(),
            cap.mutating
        );
        assert_eq!(
            cap.resolve(&ExecutionIntent::Unspecified, "foreign-os", runtime)
                .unwrap(),
            None
        );
        assert!(cap
            .resolve(&ExecutionIntent::ReadOnly, "foreign-os", runtime)
            .is_err());
        assert!(cap
            .resolve(&ExecutionIntent::Mutating, "foreign-os", runtime)
            .is_err());
    }
    let mut pty_only = cap.clone();
    pty_only.read_only.as_mut().unwrap().runtime_mode = None;
    assert!(pty_only
        .resolve(&ExecutionIntent::ReadOnly, "fixture-os", true)
        .is_err());
    assert!(pty_only
        .resolve(&ExecutionIntent::ReadOnly, "fixture-os", false)
        .is_ok());
}

#[test]
fn absent_enforcement_fails_closed_for_read_only_and_explains_the_gap() {
    let cap = ExecutionSandboxCapability {
        unsupported_reason: Some("opaque-gap".into()),
        ..Default::default()
    };
    assert!(cap
        .resolve(&ExecutionIntent::ReadOnly, "fixture-os", false)
        .unwrap_err()
        .contains("opaque-gap"));
    assert_eq!(
        cap.resolve(&ExecutionIntent::Mutating, "fixture-os", false)
            .unwrap(),
        None
    );
}

#[test]
fn declaration_round_trip_and_argument_overrides() {
    let cap = capability();
    let decoded: ExecutionSandboxCapability =
        serde_json::from_value(serde_json::to_value(&cap).unwrap()).unwrap();
    assert_eq!(decoded, cap);
    for value in ["--escape", "--escape=true", "-cevil=true"] {
        assert!(cap.validate_arguments(&[value.into()]).is_err());
    }
    assert!(cap.validate_arguments(&["--opaque".into()]).is_ok());
}
