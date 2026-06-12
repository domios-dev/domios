// Detached NSIS install helper keeps running after CLI exits.
// If continuation is set, wait for API readiness and forward text to `continue_pane`.
#[cfg(target_os = "windows")]
pub fn run_install_from_file(
    installer_path: &str,
    json_output: bool,
    continue_prompt: Option<&str>,
    continue_pane: Option<&str>,
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
    // Resume from it with `--continue-from-file` after restart.
    let continue_file = continue_prompt.map(|text| {
        let p =
            std::env::temp_dir().join(format!("codeterm-continuation-{}.txt", std::process::id()));
        let _ = std::fs::write(&p, text);
        p
    });

    let continue_block = match (continue_file.as_ref(), continue_pane) {
        (Some(file), Some(pane)) => format!(
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
            echo [self-install] daemon API never came up — skipping continue prompt >> \"%LOG%\"\r\n\
            goto cleanup\r\n\
            :api_ready\r\n\
            echo [self-install] daemon API ready, waiting 5s for shell+agent resume >> \"%LOG%\"\r\n\
            timeout /t 5 /nobreak > nul\r\n\
            \"{cli}\" send-text --pane {pane} --from-file \"{file}\" >> \"%LOG%\" 2>&1\r\n\
            echo [self-install] continue prompt delivered >> \"%LOG%\"\r\n\
            del \"{file}\" 2>nul\r\n\
            :cleanup\r\n",
            cli = cli_exe.display(),
            pane = pane,
            file = file.display(),
        ),
        _ => String::from(":cleanup\r\n"),
    };

    let bat = format!(
        "@echo off\r\n\
        set \"LOG={log}\"\r\n\
        echo [self-install] starting at %date% %time% > \"%LOG%\"\r\n\
        timeout /t 2 /nobreak > nul\r\n\
        taskkill /F /IM codeterm.exe >> \"%LOG%\" 2>&1\r\n\
        taskkill /F /IM codeterm-embed.exe >> \"%LOG%\" 2>&1\r\n\
        timeout /t 2 /nobreak > nul\r\n\
        echo [self-install] running installer >> \"%LOG%\"\r\n\
        \"{installer}\" /S >> \"%LOG%\" 2>&1\r\n\
        echo [self-install] installer exited %errorlevel% >> \"%LOG%\"\r\n\
        timeout /t 1 /nobreak > nul\r\n\
        start \"\" /B \"{target}\"\r\n\
        echo [self-install] launched {target} >> \"%LOG%\"\r\n\
        {continue_block}\
        del \"%~f0\" 2>nul\r\n",
        log = log_path.display(),
        installer = installer_path.replace('/', "\\"),
        target = target_exe.display(),
        continue_block = continue_block,
    );

    if let Err(e) = std::fs::write(&bat_path, bat) {
        eprintln!("Failed to write self-install helper: {}", e);
        std::process::exit(1);
    }

    eprintln!("[install --from-file] Installer: {}", installer_path);
    eprintln!("[install --from-file] Target:    {}", target_exe.display());
    eprintln!("[install --from-file] Log:       {}", log_path.display());
    if continue_prompt.is_some() && continue_pane.is_some() {
        eprintln!(
            "[install --from-file] Continue: will deliver prompt to pane {} after restart",
            continue_pane.unwrap_or("?")
        );
    } else if continue_prompt.is_some() {
        eprintln!("[install --from-file] WARN: --continue-prompt set but no --continue-pane / $CODETERM_PANE_ID — prompt will be DROPPED");
    }
    eprintln!("[install --from-file] Detaching. Daemon will go down in ~2s.");

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let spawn = std::process::Command::new("cmd.exe")
        .args(["/c", "start", "/B", "/MIN", "cmd.exe", "/c"])
        .arg(&bat_path)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();

    if let Err(e) = spawn {
        eprintln!("Failed to spawn self-install helper: {}", e);
        std::process::exit(1);
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

#[cfg(not(target_os = "windows"))]
pub fn run_install_from_file(
    _installer_path: &str,
    _json_output: bool,
    _continue_prompt: Option<&str>,
    _continue_pane: Option<&str>,
) {
    eprintln!("--from-file is currently Windows-only");
    std::process::exit(2);
}
