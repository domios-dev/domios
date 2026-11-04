use crate::ports::surface::SurfaceHost;
use crate::service::{agent_pad::AgentPad, ServiceContext};
use std::path::PathBuf;

impl SurfaceHost for ServiceContext {
    fn browser_upload_folders(&self, profile: &str) -> Vec<PathBuf> {
        if crate::service::browser::profile::validate_named_profile(profile).is_err() {
            return Vec::new();
        }
        self.ext::<crate::config::ConfigState>()
            .lock()
            .browser_upload_folders
            .get(profile)
            .map(|folders| folders.iter().map(PathBuf::from).collect())
            .unwrap_or_default()
    }
    fn tab_exists(&self, tab: &str) -> bool {
        self.mux.read().get_tab(tab).is_some()
    }
    fn tab_cwd(&self, tab: &str) -> Option<String> {
        self.mux.read().get_tab(tab).and_then(|tab| tab.cwd.clone())
    }
    fn tab_team(&self, tab: &str) -> Option<String> {
        self.mux
            .read()
            .get_tab(tab)
            .and_then(|tab| tab.team_id.clone())
    }
    fn agent_pad(&self, tab: &str) -> Option<PathBuf> {
        AgentPad::open(&self.agent_pad_data_dir, tab)
            .ok()
            .map(|pad| pad.path().to_path_buf())
    }
}
