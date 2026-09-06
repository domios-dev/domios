// Detached NSIS install helper keeps running after CLI exits.
#[cfg(target_os = "windows")]
fn windows_continue_block(
    cli_exe: &std::path::Path,
    continue_file: &std::path::Path,
    tab: &str,
) -> String {
    format!(
        "echo [self-install] waiting for daemon API >> \"%LOG%\"\r\n\
        set /a TRIES=0\r\n\
        :wait_api\r\n\
        curl -s -m 2 -o nul -w \"%%{{http_code}}\" http://127.0.0.1:7685/api/health > \"%TEMP%\\codeterm-health.txt\" 2>nul\r\n\
        set /p HC=<\"%TEMP%\\codeterm-health.txt\"\r\n\
        if \"%HC%\"==\"200\" goto api_ready\r\n\
        set /a TRIES+=1\r\n\
        if %TRIES% GEQ 60 goto api_timeout\r\n\
        timeout /t 1 /nobreak > nul\r\n\
        goto wait_api\r\n\
        :api_timeout\r\n\
        echo [self-install] daemon API never came up - continuation retained at {file} >> \"%LOG%\"\r\n\
        goto cleanup\r\n\
        :api_ready\r\n\
        echo [self-install] daemon API ready, awaiting acknowledged continuation submit >> \"%LOG%\"\r\n\
        \"{cli}\" send --tab {tab} --from-file \"{file}\" >> \"%LOG%\" 2>&1\r\n\
        if errorlevel 1 goto continue_failed\r\n\
        echo [self-install] continue prompt submission confirmed >> \"%LOG%\"\r\n\
        del \"{file}\" 2>nul\r\n\
        goto cleanup\r\n\
        :continue_failed\r\n\
        echo [self-install] continue prompt failed - continuation retained at {file} >> \"%LOG%\"\r\n\
        :cleanup\r\n",
        cli = cli_exe.display(),
        tab = tab,
        file = continue_file.display(),
    )
}

#[cfg(target_os = "windows")]
fn windows_runtime_env_scrub_block() -> &'static str {
    "for /f \"tokens=1 delims==\" %%V in ('set CODETERM 2^>nul') do set \"%%V=\"\r\n\
    set \"CARGO_TARGET_DIR=\"\r\n\
    set \"CARGO_INCREMENTAL=\"\r\n\
    set \"RUSTC_WRAPPER=\"\r\n\
    set \"SCCACHE_BASEDIRS=\"\r\n"
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
    continue_block: &str,
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
        {continue_block}\
        del \"%~f0\" 2>nul\r\n",
        log = log_path.display(),
        installer = installer_path.replace('/', "\\"),
        target = target_exe.display(),
        runtime_env_scrub = windows_runtime_env_scrub_block(),
    )
}

// If continuation is set, wait for API readiness and forward text to `continue_tab`.
#[cfg(target_os = "windows")]
pub fn run_install_from_file(
    installer_path: &str,
    json_output: bool,
    continue_prompt: Option<&str>,
    continue_tab: Option<&str>,
) {
    use std::io::Write;
    use std::os::windows::process::CommandExt;

    let installer = std::path::Path::new(installer_path);
    if !installer.exists() {
        eprintln!("Installer not found: {}", installer_path);
        std::process::exit(1);
    }

    let local_app_data = std::env::var("LocalAppData").unwrap_or_default();
    let target_exe = std::path::PathBuf::from(&local_app_data)
        .join("codeterm")
        .join("codeterm.exe");
    let cli_exe = dirs::home_dir()
        .map(|h| h.join(".codeterm").join("bin").join("codeterm.exe"))
        .unwrap_or_else(|| std::path::PathBuf::from("codeterm.exe"));

    let log_path = std::path::PathBuf::from(&local_app_data)
        .join("codeterm")
        .join("self-install.log");
    let bat_path =
        std::env::temp_dir().join(format!("codeterm-self-install-{}.bat", std::process::id()));

    // Use a temp file for continuation text to avoid cmd.exe quoting issues.
    let continue_file = continue_prompt.map(|text| {
        let p =
            std::env::temp_dir().join(format!("codeterm-continuation-{}.txt", std::process::id()));
        let _ = std::fs::write(&p, text);
        p
    });

    let continue_block = match (continue_file.as_ref(), continue_tab) {
        (Some(file), Some(tab)) => windows_continue_block(&cli_exe, file, tab),
        _ => String::from(":cleanup\r\n"),
    };

    let bat = windows_install_script(&log_path, installer_path, &target_exe, &continue_block);

    if let Err(e) = std::fs::write(&bat_path, bat) {
        eprintln!("Failed to write self-install helper: {}", e);
        std::process::exit(1);
    }

    eprintln!("[install --from-file] Installer: {}", installer_path);
    eprintln!("[install --from-file] Target:    {}", target_exe.display());
    eprintln!("[install --from-file] Log:       {}", log_path.display());
    if continue_prompt.is_some() && continue_tab.is_some() {
        eprintln!(
            "[install --from-file] Continue: will deliver prompt to tab {} after restart",
            continue_tab.unwrap_or("?")
        );
    } else if continue_prompt.is_some() {
        eprintln!("[install --from-file] WARN: --continue-prompt set but no --continue-tab / $CODETERM_TAB_ID — prompt will be DROPPED");
    }
    eprintln!("[install --from-file] Detaching installer helper.");

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let broker = std::process::Command::new("powershell.exe")
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
            eprintln!(
                "Failed to detach self-install helper: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to start self-install broker: {}", e);
            std::process::exit(1);
        }
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
}

#[cfg(target_os = "macos")]
pub fn run_install_from_file(
    installer_path: &str,
    json_output: bool,
    continue_prompt: Option<&str>,
    continue_tab: Option<&str>,
) {
    use std::io::Write;
    use std::os::unix::process::CommandExt;

    let src = std::path::Path::new(installer_path);
    if !src.exists() {
        eprintln!("Installer not found: {}", installer_path);
        std::process::exit(1);
    }
    let tab = continue_tab
        .map(str::to_string)
        .or_else(|| std::env::var("CODETERM_TAB_ID").ok());
    let cli_exe = std::env::current_exe().unwrap_or_else(|_| "codeterm".into());
    let tmp = std::env::temp_dir();
    let log = tmp.join("codeterm-self-install.log");
    let script = tmp.join(format!("codeterm-self-install-{}.sh", std::process::id()));

    // The continuation prompt is written to a file the detached script sends
    // to the tab once the new daemon is healthy — the session continues.
    let continue_file = continue_prompt.map(|text| {
        let p = tmp.join(format!("codeterm-continuation-{}.txt", std::process::id()));
        let _ = std::fs::write(&p, text);
        p
    });
    let continue_block = match (continue_file.as_ref(), tab.as_ref()) {
        (Some(file), Some(tab)) => format!(
            "echo '[self-install] waiting for daemon API'\n\
             for i in $(seq 1 60); do\n\
             \x20 code=$(curl -s -m 2 -o /dev/null -w '%{{http_code}}' http://127.0.0.1:7685/api/health || true)\n\
             \x20 [ \"$code\" = 200 ] && break\n\
             \x20 sleep 1\n\
             done\n\
             echo '[self-install] daemon healthy; awaiting acknowledged continuation submit'\n\
             if \"{cli}\" send --tab {tab} --from-file \"{file}\"; then\n\
             \x20 echo '[self-install] continue prompt submission confirmed'\n\
             \x20 rm -f \"{file}\"\n\
             else\n\
             \x20 echo '[self-install] continue prompt failed; continuation retained at {file}'\n\
             fi\n",
            cli = cli_exe.display(),
            tab = tab,
            file = file.display(),
        ),
        _ => String::new(),
    };

    // .dmg → mount at a space-free temp mountpoint, copy the .app out, detach;
    // .app → copy directly. The old app is rm -rf'd in the body before this,
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
         {continue_block}\
         echo '[self-install] done'\n\
         rm -f \"{script}\"\n",
        log = log.display(),
        install_block = install_block,
        continue_block = continue_block,
        script = script.display(),
    );
    if let Err(e) = std::fs::write(&script, body) {
        eprintln!("Failed to write self-install helper: {}", e);
        std::process::exit(1);
    }

    eprintln!("[install --from-file] Source: {}", installer_path);
    eprintln!("[install --from-file] Log:    {}", log.display());
    match (continue_prompt.is_some(), tab.is_some()) {
        (true, true) => eprintln!(
            "[install --from-file] Continue: prompt will reach tab {} after restart",
            tab.as_deref().unwrap_or("?")
        ),
        (true, false) => {
            eprintln!("[install --from-file] WARN: --continue-prompt set but no --continue-tab / $CODETERM_TAB_ID — it will be DROPPED")
        }
        _ => {}
    }
    eprintln!("[install --from-file] Detaching. CodeTerm goes down in ~2s.");

    // Detach into its own process group so it survives this CLI + the daemon.
    let spawn = std::process::Command::new("bash")
        .arg(&script)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if let Err(e) = spawn {
        eprintln!("Failed to spawn self-install helper: {}", e);
        std::process::exit(1);
    }
    if json_output {
        println!(
            "{{\"status\":\"helper_detached\",\"log\":\"{}\"}}",
            log.display()
        );
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{windows_continue_block, windows_helper_broker_script, windows_install_script};

    #[test]
    fn helper_is_created_by_out_of_job_windows_broker() {
        let script = windows_helper_broker_script();

        assert!(script.contains("Invoke-CimMethod -ClassName Win32_Process"));
        assert!(script.contains("CODETERM_SELF_INSTALL_HELPER"));
        assert!(script.contains("ReturnValue -ne 0"));
        assert!(!script.contains("Start-Process"));
    }

    #[test]
    fn continuation_is_deleted_only_after_acknowledged_submit() {
        let script = windows_continue_block(
            std::path::Path::new("codeterm.exe"),
            std::path::Path::new("continuation.txt"),
            "tab-1",
        );

        assert!(script.contains("if errorlevel 1 goto continue_failed"));
        assert!(script.contains("continue prompt submission confirmed"));
        assert!(script.contains("continue prompt failed - continuation retained"));
        assert!(!script.contains("waiting 5s"));
        assert!(
            script.find("if errorlevel 1").unwrap()
                < script.find("del \"continuation.txt\"").unwrap()
        );
    }

    #[test]
    fn installed_daemon_launch_scrubs_dev_and_build_environment() {
        let script = windows_install_script(
            std::path::Path::new("install.log"),
            "CodeTerm_setup.exe",
            std::path::Path::new("codeterm.exe"),
            ":cleanup\r\n",
        );

        let scrub = script.find("set CODETERM").unwrap();
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
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn run_install_from_file(
    _installer_path: &str,
    _json_output: bool,
    _continue_prompt: Option<&str>,
    _continue_tab: Option<&str>,
) {
    eprintln!("--from-file is currently Windows/macOS-only");
    std::process::exit(2);
}
