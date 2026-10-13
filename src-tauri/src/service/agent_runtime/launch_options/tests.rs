use super::*;

#[test]
fn permission_mode_never_broadens_an_explicit_false_or_an_unapproved_request() {
    assert_eq!(
        authorized_mode(Some(false), true).unwrap(),
        "auto-accept-edits"
    );
    assert_eq!(authorized_mode(Some(true), true).unwrap(), "full-access");
    assert!(authorized_mode(Some(true), false).is_err());
    assert!(authorized_mode(None, false).is_err());
}

#[test]
fn writable_roots_require_existing_directories_and_deduplicate_canonical_authority() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_path_buf();
    let validated = validate_host_directories(vec![root.clone(), root.join(".")]).unwrap();
    assert_eq!(validated.len(), 1);
    assert_eq!(
        validated[0],
        codeterm_paths::strip_verbatim_prefix(root.canonicalize().unwrap().to_str().unwrap())
    );
    assert!(validate_host_directories(vec![root.join("missing")]).is_err());
    let file = root.join("file");
    std::fs::write(&file, "opaque").unwrap();
    assert!(validate_host_directories(vec![file]).is_err());
    assert!(validate_host_directories(vec![PathBuf::from("relative")]).is_err());
}

#[test]
fn saved_options_round_trip_preserves_directories_and_explicit_network_denial() {
    let options = RestorableRuntimeLaunchOptions {
        provider_id: "fixture-provider".into(),
        binding_nonce: "fixture-nonce".into(),
        mode: "auto-accept-edits".into(),
        additional_directories: vec![std::env::current_dir().unwrap().to_str().unwrap().into()],
        network_access: Some(false),
    };
    let saved = serde_json::to_vec(&options).unwrap();
    let restored: RestorableRuntimeLaunchOptions = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored, options);
    assert_eq!(runtime_settings(&restored).network_access, Some(false));
    assert_eq!(
        runtime_settings(&restored).additional_directories,
        options.additional_directories
    );
}
