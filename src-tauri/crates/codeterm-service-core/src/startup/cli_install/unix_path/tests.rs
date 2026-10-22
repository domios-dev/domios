use super::install;

#[test]
fn a_new_shell_puts_the_cli_before_system_gui_paths_and_repeated_sourcing_is_stable() {
    let home = tempfile::Builder::new()
        .prefix("Domios owner ")
        .tempdir()
        .unwrap();
    install(home.path()).unwrap();
    let output = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            ". \"$HOME/.profile\"; . \"$HOME/.profile\"; printf '%s' \"$PATH\"",
        ])
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(output.status.success());
    let path = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        path,
        format!("{}/.domios/bin:/usr/bin:/bin", home.path().display())
    );
}

#[test]
fn install_keeps_user_profiles_and_does_not_shadow_bash_login_selection() {
    let home = tempfile::tempdir().unwrap();
    let bashrc = home.path().join(".bashrc");
    std::fs::write(&bashrc, "export OWNER_SETTING=kept").unwrap();
    install(home.path()).unwrap();
    let first = std::fs::read_to_string(&bashrc).unwrap();
    assert!(first.starts_with("export OWNER_SETTING=kept\n"));
    assert!(first.contains("$HOME/.domios/bin:$PATH"));
    assert!(!home.path().join(".bash_profile").exists());
    assert!(!home.path().join(".bash_login").exists());
    install(home.path()).unwrap();
    assert_eq!(std::fs::read_to_string(bashrc).unwrap(), first);
    assert!(std::fs::read_to_string(home.path().join(".zshenv"))
        .unwrap()
        .contains(".domios/bin"));
    assert!(
        std::fs::read_to_string(home.path().join(".config/fish/conf.d/domios.fish"))
            .unwrap()
            .contains("fish_add_path")
    );
}

#[test]
fn existing_bash_login_profiles_receive_the_cli_path_without_losing_user_content() {
    let home = tempfile::tempdir().unwrap();
    for name in [".bash_profile", ".bash_login"] {
        std::fs::write(home.path().join(name), "# User login configuration\n").unwrap();
    }
    install(home.path()).unwrap();
    for name in [".bash_profile", ".bash_login"] {
        let text = std::fs::read_to_string(home.path().join(name)).unwrap();
        assert!(text.starts_with("# User login configuration\n"));
        assert!(text.contains(".domios/bin"));
    }
}
