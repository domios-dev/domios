use super::*;

fn capability() -> ProviderSelfUpdateCapability {
    ProviderSelfUpdateCapability {
        npm_package: "@fixture/agent".into(),
        standalone_directory: "packages/standalone".into(),
        standalone_args: vec!["update".into()],
        macos_app_bundle: None,
    }
}

#[test]
fn npm_ownership_requires_matching_package_and_global_layout() {
    let metadata = r#"{"name":"@fixture/agent","version":"1.0.0"}"#;
    for (platform, prefix, root) in [
        ("windows", "/tools", "/tools/node_modules/@fixture/agent"),
        ("linux", "/tools", "/tools/lib/node_modules/@fixture/agent"),
        ("macos", "/tools", "/tools/lib/node_modules/@fixture/agent"),
    ] {
        let root = Path::new(root);
        let binary = root.join("vendor/platform/agent");
        assert_eq!(
            classify_installation(&capability(), &binary, Some((root, metadata)), platform),
            Some(Installation::Npm {
                prefix: PathBuf::from(prefix)
            })
        );
        assert_eq!(
            classify_installation(
                &capability(),
                Path::new("/other/agent"),
                Some((root, metadata)),
                platform
            ),
            None
        );
        assert_eq!(
            classify_installation(
                &capability(),
                &binary,
                Some((root, r#"{"name":"other"}"#)),
                platform
            ),
            None
        );
        assert_eq!(
            classify_installation(&capability(), &binary, Some((root, "invalid")), platform),
            None
        );
    }
    let local = Path::new("/project/node_modules/@fixture/agent");
    assert_eq!(
        classify_installation(
            &capability(),
            &local.join("bin/agent.js"),
            Some((local, metadata)),
            "linux"
        ),
        None
    );
}

#[test]
fn unknown_installations_refuse_instead_of_switching_installers() {
    for platform in ["macos", "linux", "windows"] {
        for path in [
            "/usr/local/bin/agent",
            "/opt/homebrew/bin/agent",
            "/tools/packages/standalone-extra/agent",
        ] {
            assert_eq!(
                classify_installation(&capability(), Path::new(path), None, platform),
                None
            );
        }
        assert_eq!(
            classify_installation(
                &capability(),
                Path::new("/home/user/packages/standalone/bin/agent"),
                None,
                platform
            ),
            Some(Installation::Standalone)
        );
    }
}

#[test]
fn update_commands_target_the_owning_npm_prefix_and_quote_each_shell() {
    let installation = Installation::Npm {
        prefix: PathBuf::from("/tools/owner's node"),
    };
    assert_eq!(
        update_command(&capability(), Path::new("agent"), &installation, "linux").unwrap(),
        "npm install --global --prefix '/tools/owner'\\''s node' '@fixture/agent@{version}'"
    );
    assert_eq!(
        update_command(&capability(), Path::new("agent"), &installation, "windows").unwrap(),
        "npm install --global --prefix '/tools/owner''s node' '@fixture/agent@{version}'"
    );
}

fn app_capability() -> ProviderSelfUpdateCapability {
    ProviderSelfUpdateCapability {
        macos_app_bundle: Some(MacosAppBundle {
            bundle_identifier: "com.example.agent".into(),
            team_identifier: "A1B2C3D4E5".into(),
            executable_paths: vec![
                "Contents/Resources/agent".into(),
                "Contents/Resources/agent-cli/bin/agent".into(),
                "Contents/Resources/agent-cli/AgentCLI.app/Contents/MacOS/agent".into(),
            ],
            update_guidance: "Use the app updater.".into(),
        }),
        ..capability()
    }
}

fn executable_fixture(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"fixture").unwrap();
}

#[test]
fn macos_app_ownership_requires_the_declared_entry_and_verified_enclosing_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let app = temp.path().join("packages/standalone/Renamed.app");
    let capability = app_capability();
    let owner = capability.macos_app_bundle.as_ref().unwrap();
    let canonical_app = temp
        .path()
        .canonicalize()
        .unwrap()
        .join("packages/standalone/Renamed.app");
    for entry in &owner.executable_paths {
        let cli = app.join(entry);
        executable_fixture(&cli);
        let owned = detect_installation_with(&capability, &cli, "macos", |root, declaration| {
            assert_eq!(root, canonical_app);
            assert_eq!(declaration, owner);
            true
        });
        assert_eq!(
            owned,
            Some(Installation::AppManaged {
                guidance: owner.update_guidance.clone()
            })
        );
        assert_eq!(
            update_command(&capability, &cli, &owned.unwrap(), "macos"),
            None
        );
        assert_eq!(detect_installation(&capability, &cli, "macos"), None);
    }
}

#[test]
fn regular_and_misleading_paths_keep_their_existing_installer_behavior() {
    let temp = tempfile::tempdir().unwrap();
    let capability = app_capability();
    for relative in [
        "opt/homebrew/Caskroom/agent/1.2.3/agent",
        "tools/agent",
        "Agent.app/Contents/Extras/agent",
        "Agent.app/Contents/Resources/agent-other",
    ] {
        let cli = temp.path().join(relative);
        executable_fixture(&cli);
        assert_eq!(
            detect_installation_with(&capability, &cli, "macos", |_, _| panic!("unrelated path")),
            None
        );
    }
    let lookalike = temp.path().join("Agent.app/Contents/Resources/agent");
    executable_fixture(&lookalike);
    assert_eq!(
        detect_installation_with(&capability, &lookalike, "linux", |_, _| panic!(
            "wrong platform"
        )),
        None
    );
    let standalone = temp
        .path()
        .join("agent/packages/standalone/1.2.3/bin/agent");
    executable_fixture(&standalone);
    assert_eq!(
        detect_installation(&capability, &standalone, "macos"),
        Some(Installation::Standalone)
    );
    let npm = temp.path().join("node");
    let package = npm.join("lib/node_modules/@fixture/agent");
    let npm_cli = package.join("bin/agent.js");
    executable_fixture(&npm_cli);
    std::fs::write(
        package.join("package.json"),
        br#"{"name":"@fixture/agent"}"#,
    )
    .unwrap();
    assert_eq!(
        detect_installation(&capability, &npm_cli, "macos"),
        Some(Installation::Npm {
            prefix: npm.canonicalize().unwrap()
        })
    );
}

#[cfg(unix)]
#[test]
fn app_ownership_follows_the_real_target_of_a_cli_symlink() {
    let temp = tempfile::tempdir().unwrap();
    let capability = app_capability();
    let app_entry = temp.path().join("Renamed.app/Contents/Resources/agent");
    executable_fixture(&app_entry);
    let alias = temp.path().join("agent");
    std::os::unix::fs::symlink(&app_entry, &alias).unwrap();
    assert!(matches!(
        detect_installation_with(&capability, &alias, "macos", |_, _| true),
        Some(Installation::AppManaged { .. })
    ));

    let foreign_entry = temp.path().join("foreign");
    executable_fixture(&foreign_entry);
    std::fs::remove_file(&app_entry).unwrap();
    std::os::unix::fs::symlink(&foreign_entry, &app_entry).unwrap();
    assert_eq!(
        detect_installation_with(&capability, &alias, "macos", |_, _| panic!(
            "CLI escapes bundle"
        )),
        None
    );
}

#[test]
fn declared_signing_identity_cannot_insert_an_alternative_requirement() {
    let mut owner = app_capability().macos_app_bundle.unwrap();
    assert!(owner.signing_requirement().is_some());
    owner.bundle_identifier = r#"com.example.agent" or true"#.into();
    assert_eq!(owner.signing_requirement(), None);
}
