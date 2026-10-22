use super::*;

fn model(platform: &str) -> InstalledSupervisor {
    let apps = match platform {
        "windows" => vec![PathBuf::from("C:/Apps/Domios/domios-app.exe")],
        "macos" => vec![PathBuf::from(
            "/Applications/Domios.app/Contents/MacOS/domios-app",
        )],
        _ => vec![PathBuf::from("/usr/bin/domios-app")],
    };
    InstalledSupervisor::new(
        platform,
        Some(Path::new("/home/owner")),
        &apps,
        vec![PathBuf::from("C:/Users/owner/AppData/Local/Temp")],
    )
}

fn present<'a>(paths: &'a [&'a str]) -> impl Fn(&Path) -> Option<PathBuf> + 'a {
    move |path| {
        paths
            .iter()
            .find(|candidate| path == Path::new(candidate))
            .map(|candidate| PathBuf::from(*candidate))
    }
}

#[test]
fn worker_copy_resolves_to_installed_cli_on_every_platform() {
    for (platform, installed, worker) in [
        (
            "windows",
            "C:/Apps/Domios/cli/domios.exe",
            "C:/Users/owner/AppData/Local/Temp/domios-worker-1/domios.exe",
        ),
        (
            "macos",
            "/Applications/Domios.app/Contents/Resources/cli/domios",
            "/private/var/folders/xx/domios-worker-1/domios",
        ),
        ("linux", "/usr/bin/domios", "/tmp/domios-worker-1/domios"),
    ] {
        assert_eq!(
            model(platform).resolve(None, present(&[worker, installed])),
            Ok(installed.into())
        );
        assert_eq!(
            model(platform).resolve(Some(Path::new(worker)), present(&[worker, installed])),
            Err(BrokenReason::TemporaryTarget)
        );
    }
}

#[test]
fn missing_install_refuses_even_when_worker_copy_exists() {
    for platform in ["windows", "macos", "linux"] {
        assert_eq!(
            model(platform).resolve(None, present(&["/tmp/domios-worker-1/domios"])),
            Err(BrokenReason::MissingInstall)
        );
    }
}

#[test]
fn user_install_and_cli_alias_are_recognized() {
    assert_eq!(
        model("linux").resolve(None, present(&["/home/owner/.domios/bin/codeterm"])),
        Ok("/home/owner/.domios/bin/codeterm".into())
    );
    assert_eq!(
        model("windows").resolve(None, present(&["/home/owner/.domios/bin/domios.exe"])),
        Ok("/home/owner/.domios/bin/domios.exe".into())
    );
}

#[test]
fn arbitrary_existing_override_is_not_an_install() {
    assert_eq!(
        model("linux").resolve(
            Some(Path::new("/opt/unrelated/domios")),
            present(&["/opt/unrelated/domios", "/usr/bin/domios"])
        ),
        Err(BrokenReason::NotInstalledTarget)
    );
}

#[test]
fn install_symlink_into_temporary_storage_is_refused() {
    assert_eq!(
        model("macos").resolve(None, |_| Some("/private/var/folders/xy/domios".into())),
        Err(BrokenReason::MissingInstall)
    );
}

#[test]
fn install_beneath_a_temporary_root_is_refused() {
    let install = InstalledSupervisor::new("linux", Some(Path::new("/tmp/owner")), &[], vec![]);
    assert_eq!(
        install.resolve(None, |path| Some(path.into())),
        Err(BrokenReason::MissingInstall)
    );
}

#[test]
fn windows_canonical_prefix_and_case_cannot_hide_a_temporary_target() {
    let path = Path::new(r"\\?\C:\USERS\OWNER\APPDATA\LOCAL\TEMP\domios.exe");
    assert_eq!(
        model("windows").resolve(Some(path), |_| Some(path.into())),
        Err(BrokenReason::TemporaryTarget)
    );
}

#[test]
fn missing_registered_target_is_broken_even_when_os_reports_enabled() {
    let state = AutostartState::assess(
        true,
        true,
        Some(Path::new("/gone/domios")),
        &model("linux"),
        present(&["/usr/bin/domios"]),
    );
    assert_eq!(
        state,
        AutostartState::Broken {
            reason: BrokenReason::MissingTarget,
            repair_command: "domios system enable-autostart"
        }
    );
    assert_eq!(
        state.apply(serde_json::json!({"ok":true,"loaded":true,"active":true})),
        serde_json::json!({
            "ok":true, "enabled":false, "loaded":false, "active":false,
            "status":"broken", "reason":"missing_target", "repair_command":"domios system enable-autostart"
        })
    );
}

#[test]
fn existing_worker_registration_is_broken() {
    let state = AutostartState::assess(
        true,
        true,
        Some(Path::new("/tmp/domios-worker-1/domios")),
        &model("linux"),
        present(&["/tmp/domios-worker-1/domios", "/usr/bin/domios"]),
    );
    assert_eq!(
        state,
        AutostartState::Broken {
            reason: BrokenReason::TemporaryTarget,
            repair_command: REPAIR_COMMAND
        }
    );
}

#[test]
fn status_distinguishes_disabled_inactive_and_valid_installed_target() {
    for (present_registration, enabled, expected) in [
        (false, false, AutostartState::Disabled),
        (true, false, AutostartState::Inactive),
        (true, true, AutostartState::Enabled),
    ] {
        assert_eq!(
            AutostartState::assess(
                present_registration,
                enabled,
                Some(Path::new("/usr/bin/domios")),
                &model("linux"),
                present(&["/usr/bin/domios"])
            ),
            expected
        );
    }
}

#[test]
fn malformed_or_uninstalled_registration_is_broken() {
    for (target, reason) in [
        (None, BrokenReason::InvalidRegistration),
        (
            Some(Path::new("/opt/foreign/domios")),
            BrokenReason::NotInstalledTarget,
        ),
    ] {
        assert_eq!(
            AutostartState::assess(
                true,
                true,
                target,
                &model("linux"),
                present(&["/opt/foreign/domios", "/usr/bin/domios"])
            ),
            AutostartState::Broken {
                reason,
                repair_command: REPAIR_COMMAND
            }
        );
    }
}

#[test]
fn registered_symlink_must_resolve_to_the_installed_binary() {
    let aliases = [
        Path::new("/usr/bin/domios"),
        Path::new("/home/owner/.domios/bin/domios"),
    ];
    let canonical = |path: &Path| {
        aliases
            .contains(&path)
            .then(|| PathBuf::from("/usr/lib/domios/cli/domios"))
    };
    for target in [
        PathBuf::from("/usr/bin/domios"),
        Path::new("/usr/bin").join("domios"),
        Path::new("/home/owner/.domios/bin").join("domios"),
    ] {
        assert_eq!(
            AutostartState::assess(true, true, Some(&target), &model("linux"), canonical),
            AutostartState::Enabled
        );
    }
}
