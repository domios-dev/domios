use super::{windows_helper_broker_script, windows_install_script};

fn install_script() -> String {
    windows_install_script(
        std::path::Path::new("install.log"),
        "Domios_setup.exe",
        std::path::Path::new(r"C:\Users\me\AppData\Local\Domios\domios-app.exe"),
    )
}

#[test]
fn helper_stops_only_installed_processes_and_relaunches_the_domios_client() {
    let script = install_script();

    assert!(
        script.contains(r"StartsWith('C:\Users\me\AppData\Local\Domios\', 'OrdinalIgnoreCase')")
    );
    assert!(!script.contains("taskkill /F /IM"));
    assert!(script.contains(r#"start "" "C:\Users\me\AppData\Local\Domios\domios-app.exe""#));
}

#[test]
fn helper_is_created_by_out_of_job_windows_broker() {
    let script = windows_helper_broker_script();

    assert!(script.contains("Invoke-CimMethod -ClassName Win32_Process"));
    assert!(script.contains("CODETERM_SELF_INSTALL_HELPER"));
    assert!(script.contains("ReturnValue -ne 0"));
    assert!(!script.contains("Start-Process"));
}

#[test]
fn helper_never_writes_input_to_a_tab() {
    let script = install_script();

    assert!(!script.contains(" send "));
    assert!(!script.contains("--tab"));
}

#[test]
fn installed_daemon_launch_scrubs_dev_and_build_environment() {
    let script = install_script();

    let scrub = script.find("set CODETERM ").unwrap();
    let launch = script
        .find("start \"\" \"C:\\Users\\me\\AppData\\Local\\Domios\\domios-app.exe\"")
        .unwrap();
    assert!(scrub < launch);
    for variable in [
        "CARGO_TARGET_DIR",
        "CARGO_INCREMENTAL",
        "RUSTC_WRAPPER",
        "SCCACHE_BASEDIRS",
    ] {
        assert!(script.contains(&format!("set \"{variable}=\"")));
    }
}

#[test]
fn missing_installer_returns_failure_before_the_caller_withdraws_resume() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-installer.exe");

    let result = super::run_install_from_file(missing.to_str().unwrap(), false);

    assert!(result.is_err());
}
