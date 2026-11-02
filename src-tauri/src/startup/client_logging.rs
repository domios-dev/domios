use codeterm_util::nonblocking_log::NonBlockingLog;
use std::{fs::File, io::Write, path::PathBuf};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const GENERATIONS: usize = 2;

struct RotatingLog {
    directory: PathBuf,
    file: Option<File>,
    written: u64,
}

impl RotatingLog {
    fn open(directory: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&directory)?;
        let mut writer = Self {
            directory,
            file: None,
            written: 0,
        };
        writer.reopen()?;
        Ok(writer)
    }

    fn reopen(&mut self) -> std::io::Result<()> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join("native-client.log"))?;
        self.written = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        self.file.take();
        let oldest = self
            .directory
            .join(format!("native-client.log.{GENERATIONS}"));
        match std::fs::remove_file(oldest) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        for generation in (0..GENERATIONS).rev() {
            let source = if generation == 0 {
                "native-client.log".into()
            } else {
                format!("native-client.log.{generation}")
            };
            match std::fs::rename(
                self.directory.join(source),
                self.directory
                    .join(format!("native-client.log.{}", generation + 1)),
            ) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        self.reopen()
    }
}

impl Write for RotatingLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.written.saturating_add(bytes.len() as u64) > MAX_BYTES {
            self.rotate()?;
        }
        if self.file.is_none() {
            self.reopen()?;
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("client log unavailable"))?;
        let count = file.write(bytes)?;
        self.written += count as u64;
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

struct ClientLogger(NonBlockingLog);

impl log::Log for ClientLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.target() == "native_updater" && metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_millis())
            .unwrap_or_default();
        self.0.submit(
            format!(
                "{at} pid={} {} {}\n",
                std::process::id(),
                record.level(),
                record.args()
            )
            .into_bytes()
            .into(),
        );
    }

    fn flush(&self) {}
}

pub(crate) fn initialize() {
    let writer = RotatingLog::open(codeterm_paths::data_dir().join("logs"));
    let sink =
        writer.and_then(|writer| NonBlockingLog::spawn("native-client", writer, 128, Vec::new()));
    match sink {
        Ok(sink) => {
            static LOGGER: std::sync::OnceLock<ClientLogger> = std::sync::OnceLock::new();
            let logger = LOGGER.get_or_init(|| ClientLogger(sink));
            if log::set_logger(logger).is_ok() {
                log::set_max_level(log::LevelFilter::Info);
                let previous = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    log::error!(target: "native_updater", "client_task_panic");
                    previous(info);
                }));
                log::info!(target: "native_updater", "client_started version={} build={}",
                    codeterm_build_info::version(), codeterm_build_info::sha());
            }
        }
        Err(_) => eprintln!("Native client diagnostic log unavailable"),
    }
}

#[cfg(test)]
mod tests;
