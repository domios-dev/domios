use super::*;

fn capability() -> ProviderSelfUpdateCapability {
    ProviderSelfUpdateCapability {
        npm_package: "@fixture/agent".into(),
        standalone_directory: "packages/standalone".into(),
        standalone_args: vec!["update".into()],
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
