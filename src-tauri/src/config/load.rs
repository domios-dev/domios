use std::path::Path;

use parking_lot::Mutex;

use super::schema::AppConfig;
use super::{agent_access, dropped_keys, kpi_default, normalize_runtime_defaults, resolve_theme};
use crate::persisted_state::{Loaded, StateFile};

/// Cache the parsed config, keyed by file mtime. `load_config()` runs on hot
/// paths (mesh heartbeats, request handlers), so the file is re-parsed only when
/// its mtime changes. The lock is held across the fill so a token for a
/// tokenless config is generated exactly once.
pub(super) type ConfigCache = Mutex<Option<(Option<std::time::SystemTime>, AppConfig)>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenPolicy {
    GenerateWhenMissing,
    KeepConfigured,
}

pub fn load_config() -> AppConfig {
    use std::sync::OnceLock;
    static CACHE: OnceLock<ConfigCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    match super::config_path() {
        Ok(path) => load_config_cached(&path, cache),
        Err(e) => {
            log::warn!("Cannot determine config path: {}. Using defaults.", e);
            with_generated_token(AppConfig::default()).0
        }
    }
}

pub(super) fn config_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(validate_config)
}

fn validate_config(text: &str) -> Result<(), String> {
    super::bounds::validate_config_text(text).map_err(|error| {
        log::warn!("[config] rejecting invalid config: {error}");
        error
    })
}

fn parse_config(text: &str) -> Result<(AppConfig, String), String> {
    let config = toml::from_str::<AppConfig>(text).map_err(|error| error.to_string())?;
    super::bounds::validate_app_config(&config)?;
    Ok((config, text.to_string()))
}

pub(super) fn load_config_cached(path: &Path, cache: &ConfigCache) -> AppConfig {
    let mut guard = cache.lock();
    let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    if let Some((cached_mtime, cfg)) = guard.as_ref() {
        if *cached_mtime == mtime {
            return cfg.clone();
        }
    }
    let last_known = guard.as_ref().map(|(_, cfg)| cfg.clone());
    let cfg = load_config_uncached_at(path, last_known);
    // Re-read after the fill: a generated token or a restored backup rewrites the file.
    let post_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    *guard = Some((post_mtime, cfg.clone()));
    cfg
}

fn load_config_uncached_at(path: &Path, last_known: Option<AppConfig>) -> AppConfig {
    let file = config_file(path);
    match file.load(parse_config) {
        Loaded::Parsed((config, contents)) => {
            finish_loaded(&file, config, &contents, TokenPolicy::GenerateWhenMissing)
        }
        Loaded::Recovered {
            value: (config, contents),
            ..
        } => finish_loaded(&file, config, &contents, TokenPolicy::KeepConfigured),
        Loaded::Unrecoverable { raw } => held_config(last_known, raw.as_deref()),
        Loaded::Missing if file.held().is_some() => held_config(last_known, None),
        Loaded::Missing => create_default(&file),
    }
}

fn finish_loaded(
    file: &StateFile,
    mut config: AppConfig,
    contents: &str,
    tokens: TokenPolicy,
) -> AppConfig {
    let migrated_agent_access = agent_access::migrate(&mut config, contents);
    let migrated_kpi_default = kpi_default::migrate(&mut config);
    let dropped_keys = dropped_keys::detect(contents);
    if !dropped_keys.is_empty() {
        log::warn!(
            "[config] upgrade dropped keys with no replacement, still present in {}: {}",
            file.path().display(),
            dropped_keys.join(", ")
        );
    }
    resolve_theme(&mut config);
    normalize_runtime_defaults(&mut config);
    // Mesh tab disabled → force offline. Tab is the on/off switch for the whole subsystem; flag is the runtime presence state.
    if !config.ui.mesh_tab_enabled {
        config.mesh.enabled = false;
    }
    log::info!("Config loaded from {}", file.path().display());
    let generated = if tokens == TokenPolicy::GenerateWhenMissing {
        let (with_token, generated) = with_generated_token(config);
        config = with_token;
        generated
    } else {
        false
    };
    if generated || migrated_agent_access || migrated_kpi_default {
        if let Ok(toml_str) = toml::to_string_pretty(&config) {
            if let Err(error) = file.save(&toml_str) {
                log::warn!("Failed to persist config update: {error}");
            }
        }
    }
    config
}

fn create_default(file: &StateFile) -> AppConfig {
    let (config, _) = with_generated_token(AppConfig::default());
    let toml_str = toml::to_string_pretty(&config).unwrap_or_else(|_| super::default_config_toml());
    match file.save(&toml_str) {
        Ok(()) => log::info!("Default config created at {}", file.path().display()),
        Err(e) => log::warn!("Failed to write default config: {}", e),
    }
    config
}

/// The in-memory config while `config.toml` is held after a failed load. A
/// token is never generated here: rotating it would lock out every client that
/// holds the configured one.
fn held_config(last_known: Option<AppConfig>, raw: Option<&str>) -> AppConfig {
    if let Some(config) = last_known {
        return config;
    }
    let mut config = AppConfig::default();
    if let Some(raw) = raw {
        salvage_critical_keys(raw, &mut config);
    }
    normalize_runtime_defaults(&mut config);
    if config.api.token.as_ref().map_or(true, |t| t.is_empty()) {
        log::warn!(
            "[config] no API token could be read from the preserved config; token-authenticated API calls are refused until config.toml is fixed or restored"
        );
    }
    config
}

/// Recover `api.token` and `agent.full_access` from the lines of an
/// unparseable config that still parse on their own.
pub(super) fn salvage_critical_keys(raw: &str, config: &mut AppConfig) {
    let mut table = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[[") {
            table.clear();
            continue;
        }
        if let Some(header) = trimmed.strip_prefix('[') {
            table = header
                .split(']')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string();
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let doc = if table.is_empty() {
            trimmed.to_string()
        } else {
            format!("[{table}]\n{trimmed}")
        };
        let Ok(parsed) = toml::from_str::<toml::Table>(&doc) else {
            continue;
        };
        if let Some(token) = lookup(&parsed, &["api", "token"]).and_then(toml::Value::as_str) {
            if !token.is_empty() {
                config.api.token = Some(token.to_string());
            }
        }
        if let Some(full_access) =
            lookup(&parsed, &["agent", "full_access"]).and_then(toml::Value::as_bool)
        {
            config.agent.full_access = Some(full_access);
        }
    }
}

fn lookup<'a>(table: &'a toml::Table, keys: &[&str]) -> Option<&'a toml::Value> {
    let (first, rest) = keys.split_first()?;
    let mut value = table.get(*first)?;
    for key in rest {
        value = value.as_table()?.get(*key)?;
    }
    Some(value)
}

pub(super) fn with_generated_token(mut config: AppConfig) -> (AppConfig, bool) {
    normalize_runtime_defaults(&mut config);
    if config.api.token.as_ref().map_or(true, |t| t.is_empty()) {
        let token = uuid::Uuid::new_v4().to_string().replace("-", "");
        log::info!("[config] auto-generated API token (none was configured)");
        config.api.token = Some(token);
        return (config, true);
    }
    (config, false)
}

#[cfg(test)]
mod tests;
