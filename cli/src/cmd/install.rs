#[cfg(any(test, target_os = "windows", target_os = "macos"))]
mod relaunch_env {
    pub(super) const CODETERM_RELAUNCH_PREFIX: &str = "CODETERM_";

    fn is_codeterm_relaunch_key(key: &str) -> bool {
        key.len() >= CODETERM_RELAUNCH_PREFIX.len()
            && key[..CODETERM_RELAUNCH_PREFIX.len()].eq_ignore_ascii_case(CODETERM_RELAUNCH_PREFIX)
    }

    pub(super) fn filter_relaunch_env<I, K, V>(env: I) -> Vec<(K, V)>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
    {
        env.into_iter()
            .filter(|(key, _)| !is_codeterm_relaunch_key(key.as_ref()))
            .collect()
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(super) fn apply_relaunch_env(command: &mut std::process::Command) {
        command.env_clear();
        command.envs(filter_relaunch_env(std::env::vars()));
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
use relaunch_env::apply_relaunch_env;
#[cfg(target_os = "windows")]
use relaunch_env::CODETERM_RELAUNCH_PREFIX;

// Detached NSIS install helper keeps running after CLI exits.
#[cfg(target_os = "windows")]
fn windows_runtime_env_scrub_block() -> String {
    format!(
        "for /f \"tokens=1 delims==\" %%V in ('set {prefix} 2^>nul') do set \"%%V=\"\r\n\
        set \"CARGO_TARGET_DIR=\"\r\n\
        set \"CARGO_INCREMENTAL=\"\r\n\
        set \"RUSTC_WRAPPER=\"\r\n\
        set \"SCCACHE_BASEDIRS=\"\r\n",
        prefix = CODETERM_RELAUNCH_PREFIX,
    )
}

#[cfg(target_os = "windows")]
fn windows_helper_broker_script() -> &'static str {
    "$line = 'cmd.exe /d /c \"\"' + $env:CODETERM_SELF_INSTALL_HELPER + '\"\"'; \
     $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{CommandLine=$line}; \
     if ($result.ReturnValue -ne 0) { Write-Error ('Win32_Process.Create failed: ' + $result.ReturnValue); exit 1 }"
}

#[cfg(target_os = "windows")]
fn windows_install_script(
    log_path: &std::path::Path,
    installer_path: &str,
    target_exe: &std::path::Path,
) -> String {
    format!(
        "@echo off\r\n\
        set \"LOG={log}\"\r\n\
        echo [self-install] starting at %date% %time% > \"%LOG%\"\r\n\
        taskkill /F /IM codeterm.exe >> \"%LOG%\" 2>&1\r\n\
        taskkill /F /IM codeterm-embed.exe >> \"%LOG%\" 2>&1\r\n\
        echo [self-install] running installer >> \"%LOG%\"\r\n\
        \"{installer}\" /S >> \"%LOG%\" 2>&1\r\n\
        echo [self-install] installer exited %errorlevel% >> \"%LOG%\"\r\n\
        {runtime_env_scrub}\
        start \"\" \"{target}\"\r\n\
        echo [self-install] launched {target} >> \"%LOG%\"\r\n\
        del \"%~f0\" 2>nul\r\n",
        log = log_path.display(),
        installer = installer_path.replace('/', "\\"),
        target = target_exe.display(),
        runtime_env_scrub = windows_runtime_env_scrub_block(),
    )
}

// The relaunched daemon owns every continuation; the helper only swaps and relaunches.
#[cfg(target_os = "windows")]
pub fn run_install_from_file(installer_path: &str, json_output: bool) -> Result<(), String> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;

    let installer = std::path::Path::new(installer_path);
    if !installer.exists() {
        return Err(format!("Installer not found: {installer_path}"));
    }

    let local_app_data = std::env::var("LocalAppData").unwrap_or_default();
    let target_exe = std::path::PathBuf::from(&local_app_data)
        .join("codeterm")
        .join("codeterm.exe");

    let log_path = std::path::PathBuf::from(&local_app_data)
        .join("codeterm")
        .join("self-install.log");
    let bat_path =
        std::env::temp_dir().join(format!("codeterm-self-install-{}.bat", std::process::id()));

    let bat = windows_install_script(&log_path, installer_path, &target_exe);

    if let Err(e) = std::fs::write(&bat_path, bat) {
        return Err(format!("Failed to write self-install helper: {e}"));
    }

    eprintln!("[install --from-file] Installer: {}", installer_path);
    eprintln!("[install --from-file] Target:    {}", target_exe.display());
    eprintln!("[install --from-file] Log:       {}", log_path.display());
    eprintln!("[install --from-file] Detaching installer helper.");

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut broker = std::process::Command::new("powershell.exe");
    apply_relaunch_env(&mut broker);
    let broker = broker
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            windows_helper_broker_script(),
        ])
        .env("CODETERM_SELF_INSTALL_HELPER", &bat_path)
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let broker = match broker {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return Err(format!(
                "Failed to detach self-install helper: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Err(e) => return Err(format!("Failed to start self-install broker: {e}")),
    };
    if !broker.stderr.is_empty() {
        eprintln!("{}", String::from_utf8_lossy(&broker.stderr).trim());
    }

    if json_output {
        println!(
            "{{\"status\":\"helper_detached\",\"log\":\"{}\"}}",
            log_path.display().to_string().replace('\\', "\\\\")
        );
    }

    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn run_install_from_file(installer_path: &str, json_output: bool) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::process::CommandExt;

    let src = std::path::Path::new(installer_path);
    if !src.exists() {
        return Err(format!("Installer not found: {installer_path}"));
    }
    let tmp = std::env::temp_dir();
    let log = tmp.join("codeterm-self-install.log");
    let script = tmp.join(format!("codeterm-self-install-{}.sh", std::process::id()));

    // The old app is rm -rf'd in the body before this, so ditto never overwrites a busy Mach-O (ETXTBSY on a running binary).
    let install_block = if installer_path.to_lowercase().ends_with(".dmg") {
        format!(
            "echo '[self-install] mounting dmg'\n\
             MP=$(mktemp -d)\n\
             hdiutil attach -nobrowse -readonly -mountpoint \"$MP\" \"{dmg}\"\n\
             APP=$(ls -d \"$MP\"/*.app | head -1)\n\
             echo \"[self-install] installing $APP\"\n\
             ditto \"$APP\" /Applications/CodeTerm.app\n\
             hdiutil detach \"$MP\" >/dev/null 2>&1 || true\n\
             rmdir \"$MP\" 2>/dev/null || true\n",
            dmg = installer_path,
        )
    } else {
        format!(
            "echo '[self-install] installing {app}'\n\
             ditto \"{app}\" /Applications/CodeTerm.app\n",
            app = installer_path,
        )
    };

    let body = format!(
        "#!/bin/bash\n\
         exec >> \"{log}\" 2>&1\n\
         echo \"[self-install] start $(date)\"\n\
         sleep 2\n\
         echo '[self-install] quitting CodeTerm'\n\
         osascript -e 'tell application \"CodeTerm\" to quit' >/dev/null 2>&1 || true\n\
         sleep 1\n\
         echo '[self-install] terminating any surviving process'\n\
         P='/Applications/CodeTerm.app/Contents/MacOS/codeterm'\n\
         for i in $(seq 1 20); do\n\
         \x20 pgrep -f \"$P\" >/dev/null 2>&1 || break\n\
         \x20 pkill -TERM -f \"$P\" 2>/dev/null || true\n\
         \x20 sleep 0.5\n\
         done\n\
         pkill -9 -f \"$P\" 2>/dev/null || true\n\
         sleep 1\n\
         echo '[self-install] removing old app (unlink dodges ETXTBSY)'\n\
         rm -rf /Applications/CodeTerm.app\n\
         {install_block}\
         echo '[self-install] relaunching'\n\
         open -a /Applications/CodeTerm.app\n\
         echo '[self-install] done'\n\
         rm -f \"{script}\"\n",
        log = log.display(),
        install_block = install_block,
        script = script.display(),
    );
    if let Err(e) = std::fs::write(&script, body) {
        return Err(format!("Failed to write self-install helper: {e}"));
    }

    eprintln!("[install --from-file] Source: {}", installer_path);
    eprintln!("[install --from-file] Log:    {}", log.display());
    eprintln!("[install --from-file] Detaching. Domios goes down in ~2s.");

    // Detach into its own process group so it survives this CLI + the daemon.
    let mut helper = std::process::Command::new("bash");
    apply_relaunch_env(&mut helper);
    helper
        .arg(&script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Failed to spawn self-install helper: {e}"))?;
    if json_output {
        println!(
            "{{\"status\":\"helper_detached\",\"log\":\"{}\"}}",
            log.display()
        );
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    Ok(())
}

#[cfg(test)]
mod relaunch_env_tests {
    use super::relaunch_env::filter_relaunch_env;

    #[test]
    fn filter_relaunch_env_strips_every_codeterm_variable_and_keeps_path_home() {
        let filtered = filter_relaunch_env([
            ("PATH", "/usr/bin"),
            ("HOME", "/Users/andrey"),
            ("USER", "andrey"),
            ("TMPDIR", "/tmp"),
            ("CARGO_TARGET_DIR", "/tmp/target"),
            ("CODETERM_INSTANCE", "aiderqa"),
            ("CODETERM_API_PORT", "7697"),
            ("CODETERM_API_URL", "http://127.0.0.1:7697"),
            ("CODETERM_API_TOKEN", "secret"),
            ("CODETERM_SOCKET_PATH", "/tmp/codeterm-dev-aiderqa.sock"),
            ("CODETERM_DATA_DIR", "/Users/andrey/.codeterm-dev-aiderqa"),
            ("CODETERM_DEV_IDENTIFIER", "com.codeterm.dev.aiderqa"),
            ("CODETERM_TAB_ID", "tab-1"),
            ("CODETERM_VITE_PORT", "1519"),
            ("CODETERM_CDP_PORT", "9322"),
        ]);

        assert_eq!(
            filtered,
            vec![
                ("PATH", "/usr/bin"),
                ("HOME", "/Users/andrey"),
                ("USER", "andrey"),
                ("TMPDIR", "/tmp"),
                ("CARGO_TARGET_DIR", "/tmp/target"),
            ]
        );
    }

    #[test]
    fn filter_relaunch_env_keeps_nothing_instance_specific() {
        let filtered = filter_relaunch_env([
            ("HOME", "/home/andrey"),
            ("PATH", "/bin"),
            ("CODETERM_INSTANCE", "aiderqa"),
            ("CODETERM_DEV_IDENTIFIER", "com.codeterm.dev.aiderqa"),
            ("CODETERM_DATA_DIR", "/home/andrey/.codeterm-dev-aiderqa"),
            ("CODETERM_SOCKET_PATH", "/tmp/codeterm-dev-aiderqa.sock"),
            ("CODETERM_API_PORT", "7697"),
        ]);
        let keys: Vec<&str> = filtered.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, vec!["HOME", "PATH"]);
        assert!(filtered
            .iter()
            .all(|(key, _)| !key.to_ascii_uppercase().starts_with("CODETERM_")));
    }

    #[test]
    fn filter_relaunch_env_strips_codeterm_prefix_case_insensitively() {
        let filtered = filter_relaunch_env([
            ("Path", "/bin"),
            ("Home", "/home/andrey"),
            ("codeterm_instance", "aiderqa"),
            ("CodeTerm_API_PORT", "7686"),
            ("CODETERM_tab_id", "tab-1"),
        ]);
        assert_eq!(filtered, vec![("Path", "/bin"), ("Home", "/home/andrey")]);
    }

    #[test]
    fn filter_relaunch_env_keeps_unrelated_and_short_keys() {
        let filtered = filter_relaunch_env([
            ("C", "1"),
            ("CODETERM", "keep"),
            ("CODETERM_", ""),
            ("NOT_CODETERM_INSTANCE", "keep"),
        ]);
        assert_eq!(
            filtered,
            vec![
                ("C", "1"),
                ("CODETERM", "keep"),
                ("NOT_CODETERM_INSTANCE", "keep")
            ]
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{windows_helper_broker_script, windows_install_script};

    fn install_script() -> String {
        windows_install_script(
            std::path::Path::new("install.log"),
            "CodeTerm_setup.exe",
            std::path::Path::new("codeterm.exe"),
        )
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

        let scrub = script.find("set CODETERM_").unwrap();
        let launch = script.find("start \"\" \"codeterm.exe\"").unwrap();
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
}

#[cfg(all(test, target_os = "macos"))]
mod mac_failure_tests {
    #[test]
    fn missing_installer_returns_failure_before_the_caller_withdraws_resume() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing-installer.app");

        let result = super::run_install_from_file(missing.to_str().unwrap(), false);

        assert!(result.is_err());
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn run_install_from_file(_installer_path: &str, _json_output: bool) -> Result<(), String> {
    Err("--from-file is currently Windows/macOS-only".into())
}
