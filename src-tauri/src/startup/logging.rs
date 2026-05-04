//! File + stderr logging setup with rotation.

use crate::paths;

/// Initialize file + stderr logging with rotation.
pub(crate) fn setup_logging() {
    use std::fs;

    let log_dir = paths::data_dir().join("logs");
    fs::create_dir_all(&log_dir).ok();

    let log_file = log_dir.join("codeterm.log");

    // Rotate: if log > 5MB, shift .1 -> .2 -> .3, current -> .1
    if let Ok(meta) = fs::metadata(&log_file) {
        if meta.len() > 5 * 1024 * 1024 {
            let _ = fs::remove_file(log_dir.join("codeterm.log.3"));
            let _ = fs::rename(log_dir.join("codeterm.log.2"), log_dir.join("codeterm.log.3"));
            let _ = fs::rename(log_dir.join("codeterm.log.1"), log_dir.join("codeterm.log.2"));
            let _ = fs::rename(&log_file, log_dir.join("codeterm.log.1"));
        }
    }

    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(level)
        .level_for("hyper", log::LevelFilter::Warn)
        .level_for("reqwest", log::LevelFilter::Warn)
        .level_for("tungstenite", log::LevelFilter::Warn)
        .level_for("tokio", log::LevelFilter::Warn)
        .level_for("mdns_sd", log::LevelFilter::Warn)
        .chain(std::io::stderr())
        .chain(fern::log_file(&log_file).expect("Failed to open log file"))
        .apply()
        .expect("Failed to initialize logging");

    log::info!("Logging initialized: file={}, level={:?}", log_file.display(), level);
}
