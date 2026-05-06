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

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            indexing_enabled: default_indexing_enabled(),
            watch_debounce_ms: default_debounce_ms(),
            max_files: default_max_files(),
        }
    }
}

fn default_indexing_enabled() -> bool { true }
fn default_debounce_ms() -> u64 { 1500 }
fn default_max_files() -> u64 { 50_000 }

pub fn config_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".codeterm").join("map").join("v1").join("config.json")
}

pub fn read_config(workspace_root: &Path) -> MapConfig {
    let path = config_path(workspace_root);
    let Ok(content) = std::fs::read_to_string(&path) else { return MapConfig::default(); };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn write_config(workspace_root: &Path, cfg: &MapConfig) -> std::io::Result<()> {
    let path = config_path(workspace_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(cfg).expect("serialize MapConfig");
    std::fs::write(&path, json)
}

pub fn get_value(cfg: &MapConfig, key: &str) -> Option<serde_json::Value> {
    match key {
        "indexing.enabled" => Some(serde_json::Value::Bool(cfg.indexing_enabled)),
        "watch.debounce_ms" => Some(serde_json::Value::Number(cfg.watch_debounce_ms.into())),
        "index.max_files" => Some(serde_json::Value::Number(cfg.max_files.into())),
        _ => None,
    }
}

pub fn set_value(cfg: &mut MapConfig, key: &str, raw: &str) -> Result<(), String> {
    let trimmed = raw.trim();
    match key {
        "indexing.enabled" => {
            cfg.indexing_enabled = parse_bool(trimmed)
                .ok_or_else(|| format!("indexing.enabled expects true|false, got: {raw}"))?;
        }
        "watch.debounce_ms" => {
            cfg.watch_debounce_ms = trimmed.parse::<u64>()
                .map_err(|_| format!("watch.debounce_ms expects unsigned int, got: {raw}"))?;
        }
        "index.max_files" => {
            cfg.max_files = trimmed.parse::<u64>()
                .map_err(|_| format!("index.max_files expects unsigned int, got: {raw}"))?;
        }
        _ => return Err(format!("unknown key: {key}")),
    }
    Ok(())
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = read_config(tmp.path());
        assert!(cfg.indexing_enabled);
        assert_eq!(cfg.watch_debounce_ms, 1500);
        assert_eq!(cfg.max_files, 50_000);
    }

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cfg = MapConfig::default();
        cfg.indexing_enabled = false;
        cfg.watch_debounce_ms = 3000;
        write_config(tmp.path(), &cfg).unwrap();
        let read = read_config(tmp.path());
        assert_eq!(cfg, read);
    }

    #[test]
    fn set_get_keys() {
        let mut cfg = MapConfig::default();
        set_value(&mut cfg, "indexing.enabled", "false").unwrap();
        assert!(!cfg.indexing_enabled);
        assert_eq!(get_value(&cfg, "indexing.enabled"), Some(serde_json::Value::Bool(false)));
        set_value(&mut cfg, "watch.debounce_ms", "2000").unwrap();
        assert_eq!(cfg.watch_debounce_ms, 2000);
        assert!(set_value(&mut cfg, "no.such.key", "1").is_err());
        assert!(set_value(&mut cfg, "indexing.enabled", "maybe").is_err());
    }
}
