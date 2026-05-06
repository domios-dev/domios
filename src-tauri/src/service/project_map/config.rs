use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MapConfig {
    #[serde(default = "default_indexing_enabled")]
    pub indexing_enabled: bool,
    #[serde(default = "default_debounce_ms")]
    pub watch_debounce_ms: u64,
    #[serde(default = "default_max_files")]
    pub max_files: u64,
}

fn default_indexing_enabled() -> bool { true }
fn default_debounce_ms() -> u64 { 1500 }
fn default_max_files() -> u64 { 50_000 }

pub fn default_config() -> MapConfig {
    MapConfig {
        indexing_enabled: default_indexing_enabled(),
        watch_debounce_ms: default_debounce_ms(),
        max_files: default_max_files(),
    }
}

pub fn config_path(workspace_cwd: &Path) -> PathBuf {
    workspace_cwd.join(".codeterm").join("map").join("v1").join("config.json")
}

pub fn read_config(workspace_cwd: &Path) -> MapConfig {
    let path = config_path(workspace_cwd);
    let Ok(content) = std::fs::read_to_string(&path) else { return default_config(); };
    serde_json::from_str(&content).unwrap_or_else(|_| default_config())
}

pub fn write_config(workspace_cwd: &Path, cfg: &MapConfig) -> std::io::Result<()> {
    let path = config_path(workspace_cwd);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(cfg).expect("serialize MapConfig");
    std::fs::write(&path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = read_config(tmp.path());
        assert!(cfg.indexing_enabled);
        assert_eq!(cfg.watch_debounce_ms, 1500);
    }

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = default_config();
        cfg.indexing_enabled = false;
        cfg.watch_debounce_ms = 800;
        write_config(tmp.path(), &cfg).unwrap();
        let read = read_config(tmp.path());
        assert_eq!(read, cfg);
    }
}
