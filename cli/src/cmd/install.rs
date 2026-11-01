#[cfg(any(test, target_os = "windows", target_os = "macos"))]
mod relaunch_env {
    #[cfg(target_os = "windows")]
    pub(super) const CODETERM_RELAUNCH_KEY: &str = "CODETERM";
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(super) use codeterm_paths::relaunch_env::apply_relaunch_env;
    #[cfg(test)]
    pub(super) use codeterm_paths::relaunch_env::filter_relaunch_env;
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
use relaunch_env::apply_relaunch_env;
#[cfg(target_os = "windows")]
use relaunch_env::CODETERM_RELAUNCH_KEY;

// Detached NSIS install helper keeps running after CLI exits.
#[cfg(target_os = "windows")]
fn windows_runtime_env_scrub_block() -> String {
    format!(
        "for /f \"tokens=1 delims==\" %%V in ('set {prefix} 2^>nul') do set \"%%V=\"\r\n\
        set \"CARGO_TARGET_DIR=\"\r\n\
        set \"CARGO_INCREMENTAL=\"\r\n\
        set \"RUSTC_WRAPPER=\"\r\n\
        set \"SCCACHE_BASEDIRS=\"\r\n",
        prefix = CODETERM_RELAUNCH_KEY,
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
    let install_dir = target_exe
        .parent()
        .map(|dir| dir.display().to_string())
        .unwrap_or_default()
        .replace('\'', "''");
    // Match by install path: release builds share the domios-app/domios-server image names.
    format!(
        "@echo off\r\n\
        set \"LOG={log}\"\r\n\
        echo [self-install] starting at %date% %time% > \"%LOG%\"\r\n\
        powershell.exe -NoProfile -NonInteractive -Command \"$p = Get-Process | Where-Object {{ $_.Path -and $_.Path.StartsWith('{install_dir}\\', 'OrdinalIgnoreCase') }}; $p | ForEach-Object {{ 'stopping ' + $_.Name + ' ' + $_.Id }}; $p | Stop-Process -Force; $p | Wait-Process -Timeout 15 -ErrorAction SilentlyContinue\" >> \"%LOG%\" 2>&1\r\n\
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
        .join("Domios")
        .join("domios-app.exe");

    let log_path = std::path::PathBuf::from(&local_app_data)
        .join("Domios")
        .join("self-install.log");
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create install directory: {error}"))?;
    }
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
        crate::cmd::print::print_json(&serde_json::json!({
            "status": "helper_detached", "log": log_path.display().to_string()
        }));
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
             ditto \"$APP\" /Applications/Domios.app\n\
             hdiutil detach \"$MP\" >/dev/null 2>&1 || true\n\
             rmdir \"$MP\" 2>/dev/null || true\n",
            dmg = installer_path,
        )
    } else {
        format!(
            "echo '[self-install] installing {app}'\n\
             ditto \"{app}\" /Applications/Domios.app\n",
            app = installer_path,
        )
    };

    let body = format!(
        "#!/bin/bash\n\
         exec >> \"{log}\" 2>&1\n\
         echo \"[self-install] start $(date)\"\n\
         sleep 2\n\
         echo '[self-install] quitting Domios'\n\
         osascript -e 'tell application \"Domios\" to quit' >/dev/null 2>&1 || true\n\
         sleep 1\n\
         echo '[self-install] terminating any surviving process'\n\
         P='/Applications/Domios.app/Contents/MacOS/domios-app'\n\
         for i in $(seq 1 20); do\n\
         \x20 pgrep -f \"$P\" >/dev/null 2>&1 || break\n\
         \x20 pkill -TERM -f \"$P\" 2>/dev/null || true\n\
         \x20 sleep 0.5\n\
         done\n\
         pkill -9 -f \"$P\" 2>/dev/null || true\n\
         sleep 1\n\
         echo '[self-install] removing old app (unlink dodges ETXTBSY)'\n\
         rm -rf /Applications/Domios.app\n\
         {install_block}\
         echo '[self-install] relaunching'\n\
         open -a /Applications/Domios.app\n\
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
        crate::cmd::print::print_json(&serde_json::json!({
            "status": "helper_detached", "log": log.display().to_string()
        }));
    }
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    Ok(())
}

#[cfg(test)]
#[path = "install/relaunch_env_tests.rs"]
mod relaunch_env_tests;

#[cfg(all(test, target_os = "windows"))]
#[path = "install/tests.rs"]
mod tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "install/mac_failure_tests.rs"]
mod mac_failure_tests;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn run_install_from_file(_installer_path: &str, _json_output: bool) -> Result<(), String> {
    Err("--from-file is currently Windows/macOS-only".into())
}
