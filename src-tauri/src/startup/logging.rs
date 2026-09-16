//! File + stderr logging setup with rotation.

use crate::paths;

/// Four generations at 50 MB each. Rotation is checked on every write, not only
/// at startup, because a daemon that runs for days otherwise grows one
/// generation without any bound — this is how a 95 MB rotated log happened.
const MAX_LOG_BYTES: u64 = 50 * 1024 * 1024;
const LOG_GENERATIONS: usize = 3;
const THIRD_PARTY_WARN_TARGETS: &[&str] = &["hyper_util", "reqwest", "tungstenite", "tokio", "mdns_sd"];

fn rotate(log_dir: &std::path::Path) {
    use std::fs;
    let file = log_dir.join("codeterm.log");
    let _ = fs::remove_file(log_dir.join(format!("codeterm.log.{LOG_GENERATIONS}")));
    for generation in (1..LOG_GENERATIONS).rev() {
        let _ = fs::rename(
            log_dir.join(format!("codeterm.log.{generation}")),
            log_dir.join(format!("codeterm.log.{}", generation + 1)),
        );
    }
    let _ = fs::rename(&file, log_dir.join("codeterm.log.1"));
}

/// A log sink that rotates itself. `fern::log_file` hands back a plain handle
/// that never checks its own size again, so the cap has to live in the writer.
struct RotatingLog {
    dir: std::path::PathBuf,
    file: std::fs::File,
    written: u64,
}

impl RotatingLog {
    fn open(dir: &std::path::Path) -> std::io::Result<Self> {
        let path = dir.join("codeterm.log");
        let written = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            dir: dir.to_path_buf(),
            file: std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?,
            written,
        })
    }
}

impl std::io::Write for RotatingLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written > MAX_LOG_BYTES {
            rotate(&self.dir);
            self.file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("codeterm.log"))?;
            self.written = 0;
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Initialize file + stderr logging with rotation.
pub(crate) fn setup_logging() {
    use std::fs;

    let log_dir = paths::data_dir().join("logs");
    fs::create_dir_all(&log_dir).ok();

    let log_file = log_dir.join("codeterm.log");

    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    let mut dispatch = fern::Dispatch::new()
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
        .level_for("codeterm::update", log::LevelFilter::Info)
        .level_for("codeterm::api::updater_routes", log::LevelFilter::Info)
        .chain(std::io::stderr());
    for target in THIRD_PARTY_WARN_TARGETS {
        dispatch = dispatch.level_for(*target, log::LevelFilter::Warn);
    }

    let mut file_open_err: Option<String> = None;
    match RotatingLog::open(&log_dir) {
        Ok(f) => dispatch = dispatch.chain(Box::new(f) as Box<dyn std::io::Write + Send>),
        Err(e) => file_open_err = Some(format!("{}", e)),
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("Failed to initialize logging: {}", e);
        return;
    }

    if let Some(e) = file_open_err {
        log::warn!(
            "Logging: failed to open file {} ({}). Falling back to stderr only.",
            log_file.display(),
            e
        );
    } else {
        log::info!(
            "Logging initialized: file={}, level={:?}",
            log_file.display(),
            level
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn the_total_log_budget_stays_within_two_hundred_megabytes() {
        let generations = (LOG_GENERATIONS + 1) as u64; // the live file plus its rotations
        assert!(
            generations * MAX_LOG_BYTES <= 200 * 1024 * 1024,
            "{generations} generations at {MAX_LOG_BYTES} bytes exceeds the 200 MB budget"
        );
    }

    #[test]
    fn a_writer_past_the_cap_rotates_instead_of_growing() {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("codeterm.log");
        std::fs::write(&live, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();

        let mut sink = RotatingLog::open(dir.path()).unwrap();
        sink.write_all(b"after the cap\n").unwrap();
        sink.flush().unwrap();

        assert!(
            dir.path().join("codeterm.log.1").exists(),
            "the oversized generation must be kept as .1"
        );
        let live_len = std::fs::metadata(&live).unwrap().len();
        assert!(
            live_len < MAX_LOG_BYTES,
            "the live log restarts small after rotation, got {live_len} bytes"
        );
    }

    #[test]
    fn rotation_drops_the_oldest_generation_rather_than_accumulating() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("codeterm.log"), b"live").unwrap();
        for generation in 1..=LOG_GENERATIONS {
            std::fs::write(
                dir.path().join(format!("codeterm.log.{generation}")),
                format!("gen{generation}"),
            )
            .unwrap();
        }

        rotate(dir.path());

        let extra = dir
            .path()
            .join(format!("codeterm.log.{}", LOG_GENERATIONS + 1));
        assert!(!extra.exists(), "rotation must not create a new generation");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("codeterm.log.1")).unwrap(),
            "live",
            "the live log becomes the newest generation"
        );
    }

    #[test]
    fn a_writer_under_the_cap_never_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = RotatingLog::open(dir.path()).unwrap();
        sink.write_all(b"one line\n").unwrap();
        sink.flush().unwrap();
        assert!(!dir.path().join("codeterm.log.1").exists());
    }

    #[test]
    fn log_composition_filters_the_actual_third_party_targets() {
        assert!(THIRD_PARTY_WARN_TARGETS.contains(&"hyper_util"));
        assert!(!THIRD_PARTY_WARN_TARGETS.contains(&"hyper"));
        assert!(THIRD_PARTY_WARN_TARGETS.contains(&"reqwest"));
        assert!(THIRD_PARTY_WARN_TARGETS.contains(&"tungstenite"));
        assert!(THIRD_PARTY_WARN_TARGETS.contains(&"tokio"));
        assert!(THIRD_PARTY_WARN_TARGETS.contains(&"mdns_sd"));
    }
}
