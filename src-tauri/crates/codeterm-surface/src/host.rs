use std::path::PathBuf;

pub trait SurfaceHost: Send + Sync {
    fn tab_exists(&self, tab: &str) -> bool;
    fn tab_cwd(&self, tab: &str) -> Option<String>;
    fn tab_team(&self, tab: &str) -> Option<String>;
    fn agent_pad(&self, tab: &str) -> Option<PathBuf>;
    fn browser_upload_folders(&self, _profile: &str) -> Vec<PathBuf> {
        Vec::new()
    }
}
