use super::*;

#[test]
fn recovery_requires_the_actual_failed_login_and_runtime_binding() {
    let binding = LoginBinding {
        nonce: "own-nonce".into(),
        epoch: 4,
        ..Default::default()
    };
    let state = json!({"epoch":4,"login":{"status":"failed","loginId":"own-login","error":"Sign-in did not complete (os error 1307)"}});
    assert!(validate_failure(Some("own-nonce"), Some(&state), &binding, "own-login").is_ok());
    assert!(validate_failure(Some("foreign-nonce"), Some(&state), &binding, "own-login").is_err());
    assert!(validate_failure(None, Some(&state), &binding, "own-login").is_err());
    assert!(validate_failure(Some("own-nonce"), None, &binding, "own-login").is_err());
    assert!(validate_failure(Some("own-nonce"), Some(&state), &binding, "foreign-login").is_err());
    assert!(validate_failure(Some("own-nonce"), Some(&state), &binding, "").is_err());
    for changed in [
        json!({"epoch":5,"login":state["login"]}),
        json!({"epoch":4,"login":{"status":"pending","loginId":"own-login","error":"(os error 1307)"}}),
        json!({"epoch":4,"login":{"status":"completed","loginId":"own-login","error":"(os error 1307)"}}),
        json!({"epoch":4,"login":{"status":"failed","loginId":"own-login","error":"(os error 5)"}}),
    ] {
        assert!(
            validate_failure(Some("own-nonce"), Some(&changed), &binding, "own-login").is_err()
        );
    }
}

#[test]
fn recovery_detects_only_the_invalid_owner_error() {
    assert!(owner_failure(
        "Sign-in did not complete: localized reason (os error 1307)"
    ));
    assert!(owner_failure("ERROR_INVALID_OWNER"));
    for reason in [
        "",
        "HTTP 1307",
        "Access denied (os error 5)",
        "(os error 13070)",
    ] {
        assert!(!owner_failure(reason));
    }
}

#[test]
fn recovery_requires_a_declared_leaf() {
    for name in ["auth.json", ".credentials.json"] {
        assert!(credential_leaf(name).is_ok());
    }
    for name in [
        "",
        ".",
        "..",
        "../auth.json",
        "x/auth.json",
        "x\\auth.json",
        "C:auth.json",
        "/auth.json",
        "NUL.json",
        "CON",
        "COM1",
        "auth.json.",
        "auth.json ",
        "auth\0.json",
    ] {
        assert!(credential_leaf(name).is_err(), "{name}");
    }
}

#[cfg(windows)]
#[test]
fn recovery_moves_the_file_to_a_private_dated_sibling_without_touching_other_files() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("named-account");
    std::fs::create_dir(&home).unwrap();
    let path = home.join("auth.json");
    codeterm_util::private_file::write_atomic(&path, b"opaque-credential").unwrap();
    std::fs::write(home.join("config.toml"), b"keep").unwrap();
    let backup = move_aside(&home, "auth.json", 1234, || Ok(())).unwrap();
    assert!(!path.exists());
    assert_eq!(std::fs::read(&backup).unwrap(), b"opaque-credential");
    assert_eq!(
        backup.parent().unwrap(),
        std::fs::canonicalize(&home).unwrap()
    );
    assert!(backup
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("auth.json.backup-1234-"));
    assert!(codeterm_util::private_file::owner_only(&backup).unwrap());
    assert_eq!(std::fs::read(home.join("config.toml")).unwrap(), b"keep");
    assert!(move_aside(&home, "auth.json", 1234, || Ok(())).is_err());
    assert_eq!(std::fs::read(&backup).unwrap(), b"opaque-credential");
}

#[cfg(windows)]
#[test]
fn changed_binding_does_not_move_or_create_any_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("auth.json");
    std::fs::write(&path, b"original").unwrap();
    assert!(move_aside(root.path(), "auth.json", 1234, || Err::<(), _>(
        "Binding changed".into()
    ))
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(windows)]
#[test]
fn recovery_pins_the_source_and_home_until_the_exact_file_is_moved() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let path = home.join("auth.json");
    std::fs::write(&path, b"original").unwrap();
    let backup = move_aside(&home, "auth.json", 42, || {
        assert!(std::fs::write(&path, b"replacement").is_err());
        assert!(std::fs::rename(&path, home.join("changed.json")).is_err());
        assert!(std::fs::rename(&home, root.path().join("changed-home")).is_err());
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&backup).unwrap(), b"original");
}

#[cfg(windows)]
#[test]
fn recovery_refuses_directories_and_hard_links() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("auth.json")).unwrap();
    assert!(move_aside(root.path(), "auth.json", 1, || Ok(())).is_err());
    std::fs::write(root.path().join("auth-file.json"), b"keep").unwrap();
    std::fs::hard_link(
        root.path().join("auth-file.json"),
        root.path().join("linked.json"),
    )
    .unwrap();
    assert!(move_aside(root.path(), "linked.json", 1, || Ok(())).is_err());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 3);
}

#[cfg(windows)]
#[test]
fn recovery_refuses_a_reparse_home_without_touching_its_target() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    let link = root.path().join("junction");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("auth.json"), b"untouched").unwrap();
    let mut command = std::process::Command::new("cmd");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(move_aside(&link, "auth.json", 1, || Ok(())).is_err());
    assert_eq!(
        std::fs::read(target.join("auth.json")).unwrap(),
        b"untouched"
    );
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 1);
    std::fs::remove_dir(link).unwrap();
}
