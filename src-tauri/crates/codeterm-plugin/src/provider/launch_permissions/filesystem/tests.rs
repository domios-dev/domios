use super::*;
use crate::provider::AgentProvider;

#[test]
fn declared_advisory_rules_preapprove_exact_native_tools_without_bypass() {
    let capability = crate::js_runtime::native_provider_fixture()
        .launch_permission_capability()
        .unwrap();
    for (folder, pad) in [
        ("C:/opaque friend", "C:/opaque pad"),
        ("/opaque friend", "/opaque pad"),
    ] {
        let read = capability
            .scoped_filesystem_args(true, &[], &[pad.into()])
            .unwrap();
        assert_eq!(&read[..2], ["--permission-mode", "dontAsk"]);
        for tool in ["Read", "Glob", "Grep"] {
            assert!(read.windows(2).any(|pair| pair == ["--allowedTools", tool]));
        }
        assert!(!read
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions"
                || arg == "acceptEdits"
                || arg == "Write"
                || arg == "Edit"));
        let rule = format!("Edit({}/**)", absolute_rule_path(pad).unwrap());
        assert!(read
            .windows(2)
            .any(|pair| pair == ["--allowedTools", rule.as_str()]));
        assert_eq!(
            read.iter().filter(|arg| arg.starts_with("Edit(")).count(),
            1
        );
        let folder_args = capability
            .scoped_filesystem_args(
                false,
                &[folder.into(), pad.into()],
                &[folder.into(), pad.into()],
            )
            .unwrap();
        assert!(!folder_args
            .iter()
            .any(|arg| ["Read", "Glob", "Grep", "Edit", "Write"].contains(&arg.as_str())));
        for root in [folder, pad] {
            assert!(folder_args
                .windows(2)
                .any(|pair| pair == ["--add-dir", root]));
            for tool in ["Read", "Edit"] {
                let rule = format!("{tool}({}/**)", absolute_rule_path(root).unwrap());
                assert!(folder_args
                    .windows(2)
                    .any(|pair| pair == ["--allowedTools", rule.as_str()]));
            }
        }
    }
}

#[test]
fn rule_roots_cannot_inject_patterns_or_relative_authority() {
    assert_eq!(
        absolute_rule_path("D:/Opaque/opaque pad").unwrap(),
        "//d/Opaque/opaque pad"
    );
    assert_eq!(
        absolute_rule_path("/opt/opaque pad").unwrap(),
        "//opt/opaque pad"
    );
    for bad in [
        "relative",
        "C:relative",
        "/root/../foreign",
        "/pad*",
        "/pad?",
        "/pad[0]",
        "/pad,Edit",
        "//server/share",
        r"/pad\foreign",
        "/pad\nEdit",
    ] {
        assert!(absolute_rule_path(bad).is_err(), "{bad}");
    }
    let mut cap = LaunchPermissionCapability::default();
    assert!(cap
        .scoped_filesystem_args(true, &[], &["/pad".into()])
        .is_err());
    cap.scoped_filesystem = Some(ScopedFilesystemPermissions {
        no_prompt_args: vec![],
        read_all_rules: vec![],
        read_root_rule: String::new(),
        write_root_rule: String::new(),
        additional_directory_args: vec![],
    });
    assert!(cap
        .scoped_filesystem_args(true, &[], &["/pad".into()])
        .is_err());
}
