use super::*;

const SCRIPT: &str = "$s = Join-Path $env:TEMP 'opaque-install.ps1'; Invoke-WebRequest -Uri 'https://example.test/install' -OutFile $s; try { & $s } finally { Remove-Item $s }";

fn values(policies: [Option<&str>; 4]) -> ExecutionPolicyValues {
    ExecutionPolicyValues {
        policies: policies.map(|value| value.map(str::to_owned)),
        ..Default::default()
    }
}

#[test]
fn policy_precedence_uses_first_defined_scope_not_most_restrictive() {
    for scope in 0..4 {
        let mut policies = [None; 4];
        policies[scope] = Some("RemoteSigned");
        for lower in policies.iter_mut().skip(scope + 1) {
            *lower = Some("Restricted");
        }
        assert_eq!(
            effective_policy(&values(policies)),
            (ExecutionPolicy::RemoteSigned, scope < 2)
        );
        policies[scope] = Some("AllSigned");
        assert_eq!(
            effective_policy(&values(policies)),
            (ExecutionPolicy::AllSigned, scope < 2)
        );
    }
}

#[test]
fn undefined_scopes_fall_through_to_preferences_or_restricted_default() {
    assert_eq!(
        effective_policy(&values([None; 4])),
        (ExecutionPolicy::Restricted, false)
    );
    assert_eq!(
        effective_policy(&values([Some("Undefined"); 4])),
        (ExecutionPolicy::Restricted, false)
    );
    assert_eq!(
        effective_policy(&values([
            Some("Undefined"),
            None,
            Some("remotesigned"),
            Some("AllSigned")
        ])),
        (ExecutionPolicy::RemoteSigned, false)
    );
}

#[test]
fn group_policy_disable_scripts_wins_even_without_execution_policy_value() {
    for scope in 0..2 {
        let mut snapshot = values([None, None, Some("RemoteSigned"), Some("RemoteSigned")]);
        snapshot.enable_scripts[scope] = Some(0);
        assert_eq!(
            effective_policy(&snapshot),
            (ExecutionPolicy::Restricted, true)
        );
        snapshot.enable_scripts[scope] = Some(1);
        snapshot.policies[scope] = Some("AllSigned".into());
        assert_eq!(
            effective_policy(&snapshot),
            (ExecutionPolicy::AllSigned, true)
        );
    }
}

#[test]
fn unsigned_local_script_blocking_covers_every_execution_policy() {
    for (value, blocked) in [
        ("Restricted", true),
        ("AllSigned", true),
        ("Undefined", true),
        ("RemoteSigned", false),
        ("Unrestricted", false),
        ("Bypass", false),
    ] {
        assert_eq!(
            ExecutionPolicy::parse(value).blocks_unsigned(),
            blocked,
            "{value}"
        );
    }
}

#[test]
fn downloaded_script_detection_matches_target_and_invocation_shape() {
    for command in [
        SCRIPT,
        "iwr https://example.test/install.ps1 -OutFile $installer; &$installer",
        "irm https://example.test/install -OutFile 'C:\\Temp\\setup.ps1'; & 'C:\\Temp\\setup.ps1'",
        "Invoke-WebRequest https://example.test/install -OutFile \"C:\\Temp\\setup.ps1\"; powershell.exe -File \"C:\\Temp\\setup.ps1\"",
        "IWR https://example.test/install.ps1 -OUTFILE $script; . $script",
    ] {
        assert!(runs_downloaded_script(command), "{command}");
    }
    for command in [
        "irm https://example.test/install.ps1 | iex",
        "curl https://example.test/install.sh | bash",
        "opaque update",
        "& 'C:\\Temp\\local.ps1'",
        "iwr https://example.test/install.ps1 -OutFile $s; & $something_else",
        "iwr https://example.test/install.ps1 -OutFile $s; & { Write-Output $s }",
        "iwr https://example.test/archive.zip -OutFile $s; & $s",
    ] {
        assert!(!runs_downloaded_script(command), "{command}");
    }
}

#[test]
fn bundled_script_file_installers_are_detected_without_provider_identity() {
    for manifest in [
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-claude/plugin.json"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-codex/plugin.json"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../plugins/provider-cursor/plugin.json"
        )),
    ] {
        let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
        assert!(runs_downloaded_script(
            manifest["installer"]["windows"].as_str().unwrap()
        ));
        for platform in ["macos", "linux"] {
            assert!(!runs_downloaded_script(
                manifest["installer"][platform].as_str().unwrap()
            ));
        }
    }
}

#[test]
fn blocking_policy_becomes_missing_prerequisite_and_recheck_clears_it() {
    let mut snapshot = values([None; 4]);
    let blocked = prerequisite(SCRIPT, &snapshot).unwrap();
    assert_eq!(blocked.installed, Some(false));
    assert!(!blocked.binary.is_empty());
    assert!(blocked
        .hint
        .contains("Set-ExecutionPolicy -Scope CurrentUser RemoteSigned"));
    assert!(super::super::missing_prerequisite_hint(&[blocked]).is_some());
    snapshot.policies[2] = Some("RemoteSigned".into());
    assert_eq!(prerequisite(SCRIPT, &snapshot), None);
    assert_eq!(prerequisite("opaque update", &values([None; 4])), None);
    snapshot.policies[0] = Some("AllSigned".into());
    assert!(prerequisite(SCRIPT, &snapshot).is_some());
}

#[cfg(not(windows))]
#[test]
fn observing_script_command_on_non_windows_adds_no_prerequisite() {
    assert!(super::super::observe(Some(SCRIPT)).is_empty());
}
