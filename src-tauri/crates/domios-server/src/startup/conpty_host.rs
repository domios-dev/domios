pub fn setup_conpty_host(resources: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x64"
        };
        let dir = resources.join("conpty").join(arch);
        if !dir.join("conpty.dll").is_file() || !dir.join("OpenConsole.exe").is_file() {
            return Err(format!(
                "Bundled ConPTY resources missing at {}",
                dir.display()
            ));
        }
        std::env::set_var("CODETERM_CONPTY_DIR", &dir);
        codeterm_service_core::startup::reap_orphaned_hosts_in_background();
    }
    #[cfg(not(windows))]
    let _ = resources;
    Ok(())
}
