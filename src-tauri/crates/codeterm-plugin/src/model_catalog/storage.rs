use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use super::{
    honest_verdicts, seed_fingerprint, BinaryFingerprint, CatalogFile, ModelInfo, ProviderModels,
    ReasoningEffortInfo, CACHE_SCHEMA_VERSION,
};

pub(super) struct ProviderOverride {
    pub(super) default: Option<String>,
    pub(super) replace: bool,
    pub(super) models: Vec<ModelInfo>,
    pub(super) default_effort: Option<String>,
    pub(super) efforts: Vec<ReasoningEffortInfo>,
}

pub fn models_dir() -> PathBuf {
    let dir = codeterm_paths::data_dir().join("models");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub(super) fn override_path(provider_id: &str) -> PathBuf {
    models_dir().join(format!("{}.toml", provider_id))
}

pub(super) fn cache_path(provider_id: &str) -> PathBuf {
    models_dir().join(format!("{}.cache.json", provider_id))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CachedSnapshot {
    #[serde(default)]
    pub(super) schema_version: u32,
    pub(super) fetched_at_unix: u64,
    #[serde(default)]
    pub(super) default: Option<String>,
    pub(super) models: Vec<ModelInfo>,
    #[serde(default)]
    pub(super) seed_fingerprint: Option<String>,
    #[serde(default)]
    pub(super) binary: Option<BinaryFingerprint>,
    #[serde(default)]
    pub(super) credentials: Vec<BinaryFingerprint>,
}

impl CachedSnapshot {
    pub(super) fn fetched_at_system(&self) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(self.fetched_at_unix))
    }
}

pub(super) fn persist_cache(provider_id: &str, snapshot: &ProviderModels) {
    let Some(fetched_at) = snapshot.fetched_at else {
        return;
    };
    let Ok(secs) = fetched_at.duration_since(UNIX_EPOCH) else {
        return;
    };
    let payload = CachedSnapshot {
        schema_version: CACHE_SCHEMA_VERSION,
        fetched_at_unix: secs.as_secs(),
        default: snapshot.default.clone(),
        models: honest_verdicts(&snapshot.models),
        seed_fingerprint: Some(seed_fingerprint(provider_id)),
        binary: snapshot.binary.clone(),
        credentials: snapshot.credentials.clone(),
    };
    let path = cache_path(provider_id);
    match serde_json::to_string_pretty(&payload) {
        Ok(json) => {
            if let Err(error) = std::fs::write(&path, json) {
                log::warn!(
                    "[model_catalog] could not write {}: {}",
                    path.display(),
                    error
                );
            }
        }
        Err(error) => log::warn!(
            "[model_catalog] could not serialize {} cache: {}",
            provider_id,
            error
        ),
    }
}

pub(super) fn read_cache(provider_id: &str) -> Option<CachedSnapshot> {
    let path = cache_path(provider_id);
    let raw = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str::<CachedSnapshot>(&raw) {
        Ok(cached) => Some(cached),
        Err(error) => {
            log::warn!(
                "[model_catalog] {} parse error — ignoring cache: {}",
                path.display(),
                error
            );
            None
        }
    }
}

#[derive(Clone)]
pub(super) struct CachedOverride {
    parsed: Option<std::sync::Arc<ProviderOverrideOwned>>,
    mtime: Option<SystemTime>,
}

#[derive(Clone)]
struct ProviderOverrideOwned {
    default: Option<String>,
    replace: bool,
    models: Vec<ModelInfo>,
    default_effort: Option<String>,
    efforts: Vec<ReasoningEffortInfo>,
}

pub(super) fn override_cache() -> &'static RwLock<HashMap<String, CachedOverride>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedOverride>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(super) fn load_override(provider_id: &str) -> Option<ProviderOverride> {
    let path = override_path(provider_id);
    let current_mtime = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok();

    if let Some(cached) = override_cache().read().get(provider_id) {
        if cached.mtime == current_mtime {
            return cached
                .parsed
                .as_ref()
                .map(|override_model| ProviderOverride {
                    default: override_model.default.clone(),
                    replace: override_model.replace,
                    models: override_model.models.clone(),
                    default_effort: override_model.default_effort.clone(),
                    efforts: override_model.efforts.clone(),
                });
        }
    }

    let parsed_arc: Option<std::sync::Arc<ProviderOverrideOwned>> = {
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        if raw.is_empty() {
            None
        } else {
            match toml::from_str::<CatalogFile>(&raw) {
                Err(error) => {
                    log::warn!(
                        "[model_catalog] {} parse error — falling back to plugin defaults: {}",
                        path.display(),
                        error
                    );
                    None
                }
                Ok(file) if file.models.is_empty() && file.efforts.is_empty() && !file.replace => {
                    log::warn!(
                        "[model_catalog] {} has no `[[models]]` or `[[efforts]]` entries — falling back to plugin defaults",
                        path.display()
                    );
                    None
                }
                Ok(file) => Some(std::sync::Arc::new(ProviderOverrideOwned {
                    default: file.default,
                    replace: file.replace,
                    models: file
                        .models
                        .into_iter()
                        .map(|model| ModelInfo {
                            default_reasoning_effort: None,
                            id: model.id,
                            display_name: model.display_name,
                            description: model.description,
                            available: model.available,
                            availability: None,
                            featured: model.featured,
                            badge: model.badge,
                            group: model.group,
                            reasoning_efforts: None,
                        })
                        .collect(),
                    default_effort: file.default_effort,
                    efforts: file
                        .efforts
                        .into_iter()
                        .map(|effort| ReasoningEffortInfo {
                            id: effort.id,
                            display_name: effort.display_name,
                            description: effort.description,
                        })
                        .collect(),
                })),
            }
        }
    };

    let cached_mtime = std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok();
    override_cache().write().insert(
        provider_id.to_string(),
        CachedOverride {
            parsed: parsed_arc.clone(),
            mtime: cached_mtime,
        },
    );

    parsed_arc.map(|override_model| ProviderOverride {
        default: override_model.default.clone(),
        replace: override_model.replace,
        models: override_model.models.clone(),
        default_effort: override_model.default_effort.clone(),
        efforts: override_model.efforts.clone(),
    })
}
