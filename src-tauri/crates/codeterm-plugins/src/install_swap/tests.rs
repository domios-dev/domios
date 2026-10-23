use super::*;

#[test]
fn failed_activation_restores_old_version() {
    let mut installed = Some("old");
    let mut backup = None;
    let mut steps = Vec::new();
    let error = swap(
        true,
        |step| {
            steps.push(step);
            match step {
                Step::Backup => backup = installed.take(),
                Step::Activate => return Err(io::Error::other("activation refused")),
                Step::Restore => installed = backup.take(),
                Step::Cleanup => panic!("must retain old version"),
            }
            Ok(())
        },
        |_| false,
    )
    .unwrap_err();
    assert_eq!(installed, Some("old"));
    assert_eq!(backup, None);
    assert_eq!(steps, [Step::Backup, Step::Activate, Step::Restore]);
    assert_eq!(
        error,
        "rename staging into place (attempts 1): activation refused; previous plugin restored"
    );
}

#[test]
fn failures_name_the_exact_step() {
    for (failed, expected) in [
        (Step::Backup, "move previous plugin aside: refused"),
        (
            Step::Restore,
            "rename staging into place (attempts 1): refused; restore previous plugin: refused",
        ),
    ] {
        let error = swap(
            true,
            |step| {
                if step == failed || (failed == Step::Restore && step == Step::Activate) {
                    Err(io::Error::other("refused"))
                } else {
                    Ok(())
                }
            },
            |_| false,
        )
        .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[test]
fn retry_is_bounded_and_only_for_selected_errors() {
    for retry in [false, true] {
        let mut attempts = 0;
        let mut restored = false;
        swap(
            true,
            |step| {
                if step == Step::Activate {
                    attempts += 1;
                    return Err(io::Error::other("busy"));
                }
                restored |= step == Step::Restore;
                Ok(())
            },
            |_| retry,
        )
        .unwrap_err();
        assert_eq!(attempts, if retry { 4 } else { 1 });
        assert!(restored);
    }
}

#[test]
fn transient_failure_can_succeed_without_rollback() {
    let mut steps = Vec::new();
    swap(
        true,
        |step| {
            steps.push(step);
            if steps == [Step::Backup, Step::Activate] {
                Err(io::Error::from_raw_os_error(32))
            } else {
                Ok(())
            }
        },
        |_| true,
    )
    .unwrap();
    assert_eq!(
        steps,
        [Step::Backup, Step::Activate, Step::Activate, Step::Cleanup]
    );
}

#[test]
fn windows_retry_classification_is_specific() {
    for code in [5, 32, 33] {
        assert_eq!(
            windows_sharing_error(&io::Error::from_raw_os_error(code)),
            cfg!(windows)
        );
    }
    assert!(!windows_sharing_error(&io::Error::from_raw_os_error(2)));
    assert!(!windows_sharing_error(&io::Error::other("access denied")));
}

#[test]
fn filesystem_rollback_preserves_payload_and_configuration() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("plugin");
    let backup = root.path().join("backup");
    let staging = root.path().join("staging");
    std::fs::create_dir(&destination).unwrap();
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(destination.join("plugin.json"), "old payload").unwrap();
    std::fs::write(destination.join("config.json"), "private configuration").unwrap();
    std::fs::write(staging.join("plugin.json"), "new payload").unwrap();
    let error = swap(
        true,
        |step| match step {
            Step::Backup => std::fs::rename(&destination, &backup),
            Step::Activate => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture refusal",
            )),
            Step::Restore => std::fs::rename(&backup, &destination),
            Step::Cleanup => std::fs::remove_dir_all(&backup),
        },
        |_| false,
    )
    .unwrap_err();
    assert_eq!(
        std::fs::read_to_string(destination.join("plugin.json")).unwrap(),
        "old payload"
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("config.json")).unwrap(),
        "private configuration"
    );
    assert!(staging.join("plugin.json").exists());
    assert!(!backup.exists());
    assert!(error.ends_with("previous plugin restored"));
}

#[test]
fn fresh_install_only_activates() {
    let mut steps = Vec::new();
    swap(
        false,
        |step| {
            steps.push(step);
            Ok(())
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(steps, [Step::Activate]);
}

#[test]
fn cleanup_sharing_violation_keeps_successful_activation() {
    let mut installed = "old";
    let mut retained_backup = false;
    let mut steps = Vec::new();
    swap(
        true,
        |step| {
            steps.push(step);
            match step {
                Step::Backup => retained_backup = true,
                Step::Activate => installed = "new",
                Step::Cleanup => return Err(io::Error::from_raw_os_error(32)),
                Step::Restore => panic!("activation succeeded"),
            }
            Ok(())
        },
        |_| false,
    )
    .unwrap();
    assert_eq!(installed, "new");
    assert!(retained_backup);
    assert_eq!(steps, [Step::Backup, Step::Activate, Step::Cleanup]);
}
