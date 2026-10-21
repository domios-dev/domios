use crate::host::SurfaceHost;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct RecordingSurfaceHost {
    pub pads: BTreeMap<String, PathBuf>,
    pub calls: Mutex<Vec<String>>,
}
impl SurfaceHost for RecordingSurfaceHost {
    fn tab_exists(&self, tab: &str) -> bool {
        self.calls.lock().push(format!("tab_exists:{tab}"));
        self.pads.contains_key(tab)
    }
    fn tab_cwd(&self, tab: &str) -> Option<String> {
        self.calls.lock().push(format!("tab_cwd:{tab}"));
        None
    }
    fn tab_team(&self, tab: &str) -> Option<String> {
        self.calls.lock().push(format!("tab_team:{tab}"));
        None
    }
    fn agent_pad(&self, tab: &str) -> Option<PathBuf> {
        self.calls.lock().push(format!("agent_pad:{tab}"));
        self.pads.get(tab).cloned()
    }
}
