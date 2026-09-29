use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incident {
    pub path: PathBuf,
    pub error: String,
    pub preserved: Option<PathBuf>,
    pub preserve_error: Option<String>,
    pub recovered_from: Option<PathBuf>,
    pub held: bool,
}

impl Incident {
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn kept_copy(&self) -> String {
        match (&self.preserved, &self.preserve_error) {
            (Some(copy), _) => format!("kept as {}", copy.display()),
            (None, Some(reason)) => format!("left in place (could not move it aside: {reason})"),
            (None, None) => "left in place".to_string(),
        }
    }

    pub fn outcome(&self) -> String {
        match (&self.recovered_from, self.held) {
            (Some(backup), false) => format!("restored from backup {}", backup.display()),
            (Some(backup), true) => format!(
                "running on backup {} in memory; nothing is written to {} until it loads again",
                backup.display(),
                self.path.display()
            ),
            (None, true) => format!(
                "no valid backup, running on defaults in memory; nothing is written to {} until you fix or restore it",
                self.path.display()
            ),
            (None, false) => "the current in-memory state is written in its place".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub path: PathBuf,
    pub preserved: Option<PathBuf>,
}

impl Hold {
    pub fn refusal(&self) -> String {
        let kept = self
            .preserved
            .as_ref()
            .map(|copy| format!("its original is kept as {}", copy.display()))
            .unwrap_or_else(|| "its original is left in place".to_string());
        format!(
            "{} failed to load and {kept}; saving is refused until the file is fixed or restored",
            self.path.display()
        )
    }
}

type Sink = Arc<dyn Fn(&Incident) + Send + Sync>;

#[derive(Default)]
struct Registry {
    holds: HashMap<PathBuf, Hold>,
    pending: Vec<Incident>,
    sink: Option<Sink>,
    reported: Vec<Incident>,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

pub fn held(path: &Path) -> Option<Hold> {
    registry().lock().holds.get(path).cloned()
}

pub fn release_hold(path: &Path) {
    if registry().lock().holds.remove(path).is_some() {
        log::info!("[state] {} loads again; saving resumed", path.display());
    }
}

/// Incidents raised before a sink exists are queued and flushed on install.
pub fn set_incident_sink(sink: impl Fn(&Incident) + Send + Sync + 'static) {
    let sink: Sink = Arc::new(sink);
    let pending = {
        let mut registry = registry().lock();
        registry.sink = Some(sink.clone());
        std::mem::take(&mut registry.pending)
    };
    for incident in &pending {
        sink(incident);
    }
}

pub(super) fn report(incident: Incident) {
    log::error!(
        "[state] {} failed to load ({}); original {}; {}",
        incident.path.display(),
        incident.error,
        incident.kept_copy(),
        incident.outcome()
    );
    let sink = {
        let mut registry = registry().lock();
        registry.reported.push(incident.clone());
        if incident.held {
            registry.holds.insert(
                incident.path.clone(),
                Hold {
                    path: incident.path.clone(),
                    preserved: incident.preserved.clone(),
                },
            );
        }
        match registry.sink.clone() {
            Some(sink) => sink,
            None => {
                registry.pending.push(incident);
                return;
            }
        }
    };
    sink(&incident);
}

/// Every incident this process raised for `path`, oldest first.
pub fn incidents_for(path: &Path) -> Vec<Incident> {
    registry()
        .lock()
        .reported
        .iter()
        .filter(|incident| incident.path == path)
        .cloned()
        .collect()
}
