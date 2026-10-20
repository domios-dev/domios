//! The client's own startup trail, kept in release builds so a window stuck on its
//! starting page can be explained from `<data>/logs/client-startup.log`.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LINES: usize = 200;
const MAX_ERROR_CHARS: usize = 600;

#[derive(Default)]
struct TrailModel {
    lines: usize,
    paused: bool,
    pending: Vec<String>,
}

impl TrailModel {
    fn record(&mut self, line: String) -> Option<Vec<String>> {
        if self.lines >= MAX_LINES {
            return None;
        }
        self.lines += 1;
        if self.paused {
            self.pending.push(line);
            Some(Vec::new())
        } else if self.pending.is_empty() {
            Some(vec![line])
        } else {
            self.pending.push(line);
            Some(std::mem::take(&mut self.pending))
        }
    }

    fn pause(&mut self) {
        self.paused = true;
    }

    fn resume(&mut self) -> Vec<String> {
        self.paused = false;
        std::mem::take(&mut self.pending)
    }

    fn requeue(&mut self, lines: Vec<String>) {
        self.pending = lines
            .into_iter()
            .chain(std::mem::take(&mut self.pending))
            .collect();
    }
}

struct Trail {
    file: Option<File>,
    model: TrailModel,
}

impl Trail {
    fn new() -> Self {
        Self {
            file: log_path().and_then(|path| open_at(&path, true)),
            model: TrailModel::default(),
        }
    }
}

static TRAIL: Mutex<Option<Trail>> = Mutex::new(None);

fn log_path() -> Option<std::path::PathBuf> {
    let paths = codeterm_paths::instance_identity::ResolvedInstancePaths::current().ok()?;
    Some(paths.data().join("logs/client-startup.log"))
}

fn open_at(path: &std::path::Path, truncate: bool) -> Option<File> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if truncate {
        options.truncate(true);
    } else {
        options.append(true);
    }
    options.open(path).ok()
}

fn write_lines(trail: &mut Trail, path: Option<&std::path::Path>, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    if trail.file.is_none() {
        trail.file = path.and_then(|path| open_at(path, false));
    }
    let Some(file) = trail.file.as_mut() else {
        trail.model.requeue(lines);
        return;
    };
    for line in lines {
        let _ = writeln!(file, "{line}");
    }
    let _ = file.flush();
}

pub(super) struct ServerLaunchTrailPause;

pub(super) fn pause_for_server_launch() -> ServerLaunchTrailPause {
    let mut guard = TRAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let trail = guard.get_or_insert_with(Trail::new);
    trail.model.pause();
    trail.file.take();
    ServerLaunchTrailPause
}

impl Drop for ServerLaunchTrailPause {
    fn drop(&mut self) {
        let path = log_path();
        let mut guard = TRAIL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(trail) = guard.as_mut() else {
            return;
        };
        let lines = trail.model.resume();
        if trail.file.is_none() {
            trail.file = path.as_deref().and_then(|path| open_at(path, false));
        }
        write_lines(trail, path.as_deref(), lines);
    }
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
    let path = log_path();
    let mut guard = TRAIL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let trail = guard.get_or_insert_with(Trail::new);
    let Some(lines) = trail.model.record(line.clone()) else {
        return;
    };
    #[cfg(not(debug_assertions))]
    let _ = writeln!(std::io::stderr().lock(), "DOMIOS_CLIENT_STARTUP {line}");
    write_lines(trail, path.as_deref(), lines);
}

#[cfg(test)]
mod tests;
