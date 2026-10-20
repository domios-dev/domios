#[tauri::command]
pub async fn reveal_file(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&path);
        if !path.is_absolute() || !path.is_file() {
            return Err("File is no longer available".into());
        }
        #[cfg(target_os = "windows")]
        let (program, args) = ("explorer", vec![format!("/select,{}", path.display())]);
        #[cfg(target_os = "macos")]
        let (program, args) = (
            "open",
            vec!["-R".into(), path.to_string_lossy().into_owned()],
        );
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let (program, args) = (
            "xdg-open",
            vec![path
                .parent()
                .ok_or("File has no parent directory")?
                .to_string_lossy()
                .into_owned()],
        );
        let mut command = std::process::Command::new(program);
        codeterm_service_core::process_util::no_window_std(&mut command);
        command
            .args(args)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Unable to reveal file: {error}"))
    })
    .await
    .map_err(|error| error.to_string())?
}
