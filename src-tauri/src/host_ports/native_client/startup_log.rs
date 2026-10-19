//! The client's own startup trail, kept in release builds so a window stuck on its
//! starting page can be explained from `<data>/logs/client-startup.log`.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LINES: usize = 200;
const MAX_ERROR_CHARS: usize = 300;

struct Trail {
    file: Option<File>,
    lines: usize,
}

static TRAIL: Mutex<Option<Trail>> = Mutex::new(None);

fn open() -> Option<File> {
    let paths = codeterm_paths::instance_identity::ResolvedInstancePaths::current().ok()?;
    let dir = paths.data().join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join("client-startup.log"))
        .ok()
}

pub(super) fn record(stage: &str, status: &str, error: Option<&str>, exit_code: Option<i32>) {
    let at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let mut line = format!("{at_ms} pid={} {stage} {status}", std::process::id());
    if let Some(code) = exit_code {
        line.push_str(&format!(" exit={code}"));
    }
    if let Some(error) = error {
        line.push_str(": ");
        line.extend(error.chars().take(MAX_ERROR_CHARS));
    }
    let mut guard = TRAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let trail = guard.get_or_insert_with(|| Trail {
        file: open(),
        lines: 0,
    });
    if trail.lines >= MAX_LINES {
        return;
    }
    trail.lines += 1;
    #[cfg(not(debug_assertions))]
    let _ = writeln!(std::io::stderr().lock(), "DOMIOS_CLIENT_STARTUP {line}");
    if let Some(file) = trail.file.as_mut() {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}
