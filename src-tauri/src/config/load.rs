use std::path::Path;

use parking_lot::Mutex;

use super::schema::AppConfig;
use super::{agent_access, dropped_keys, kpi_default, normalize_runtime_defaults, resolve_theme};
use crate::persisted_state::{Loaded, StateFile};

pub(super) const SALVAGED_CONFIG_MARKER: &str =
    "# CodeTerm salvaged config; remove this marker after reviewing or restoring the corrupt copy.";

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
    if let Ok(contents) = std::fs::read_to_string(path) {
        if is_salvaged_config(&contents) {
            file.hold_for_repair();
            if let Ok((config, contents)) = parse_config(&contents) {
                return finish_loaded(&file, config, &contents, TokenPolicy::KeepConfigured);
            }
        }
    }
    match file.load(parse_config) {
        Loaded::Parsed((config, contents)) => {
            finish_loaded(&file, config, &contents, TokenPolicy::GenerateWhenMissing)
        }
        Loaded::Recovered {
            value: (config, contents),
            ..
        } => finish_loaded(&file, config, &contents, TokenPolicy::KeepConfigured),
        Loaded::Unrecoverable { raw } => held_config(&file, last_known, raw.as_deref()),
        Loaded::Missing if file.held().is_some() => held_config(&file, last_known, None),
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
fn held_config(file: &StateFile, last_known: Option<AppConfig>, raw: Option<&str>) -> AppConfig {
    let (mut config, contents) = match (last_known, raw) {
        (Some(config), _) => {
            let contents = toml::to_string_pretty(&config)
                .ok()
                .map(|contents| mark_salvaged_config(&contents));
            (config, contents)
        }
        (None, Some(raw)) => {
            let (config, contents) = salvage_config(raw)
                .unwrap_or_else(|| (AppConfig::default(), mark_salvaged_config("")));
            (config, Some(contents))
        }
        (None, None) => (AppConfig::default(), None),
    };
    if let Some(contents) = contents {
        if let Err(error) = file.create_held_recovery(&contents) {
            log::warn!("[config] could not write salvaged config: {error}");
        }
    }
    normalize_runtime_defaults(&mut config);
    if config.api.token.as_ref().map_or(true, |t| t.is_empty()) {
        log::warn!(
            "[config] no API token could be read from the preserved config; token-authenticated API calls are refused until config.toml is fixed or restored"
        );
    }
    config
}

pub(super) fn is_salvaged_config(contents: &str) -> bool {
    contents.lines().next() == Some(SALVAGED_CONFIG_MARKER)
}

pub(super) fn mark_salvaged_config(contents: &str) -> String {
    format!("{SALVAGED_CONFIG_MARKER}\n{contents}")
}

fn salvage_config(raw: &str) -> Option<(AppConfig, String)> {
    let mut tables = salvage_tables(raw);
    let (token, full_access) = salvaged_critical_values(raw);
    if let Some(token) = token {
        insert_nested_value(&mut tables, &["api", "token"], toml::Value::String(token));
    }
    if let Some(full_access) = full_access {
        insert_nested_value(
            &mut tables,
            &["agent", "full_access"],
            toml::Value::Boolean(full_access),
        );
    }
    let body = toml::to_string_pretty(&toml::Value::Table(tables)).ok()?;
    let contents = mark_salvaged_config(&body);
    let config = parse_config(&contents).ok()?.0;
    Some((config, contents))
}

fn salvage_tables(raw: &str) -> toml::Table {
    let mut tables = toml::Table::new();
    let mut fragment = String::new();
    for line in raw.lines() {
        if is_table_header(line) && !fragment.trim().is_empty() {
            merge_fragment(&mut tables, &fragment);
            fragment.clear();
        }
        fragment.push_str(line);
        fragment.push('\n');
    }
    merge_fragment(&mut tables, &fragment);
    tables
}

fn is_table_header(line: &str) -> bool {
    let line = line.trim();
    if !line.starts_with('[') {
        return false;
    }
    toml::from_str::<toml::Value>(line).is_ok_and(|parsed| {
        parsed
            .as_table()
            .is_some_and(|root| !root.is_empty() && root.values().all(empty_section_value))
    })
}

fn empty_section_value(value: &toml::Value) -> bool {
    match value {
        toml::Value::Table(table) => table.values().all(empty_section_value),
        toml::Value::Array(array) => array.iter().all(empty_section_value),
        _ => false,
    }
}

fn merge_fragment(tables: &mut toml::Table, fragment: &str) {
    if toml::from_str::<AppConfig>(fragment).is_err() {
        return;
    }
    let Ok(parsed) = toml::from_str::<toml::Value>(fragment) else {
        return;
    };
    if let toml::Value::Table(parsed) = parsed {
        merge_tables(tables, parsed);
    }
}

fn merge_tables(target: &mut toml::Table, incoming: toml::Table) {
    for (key, value) in incoming {
        if let Some(existing) = target.get_mut(&key) {
            merge_values(existing, value);
        } else {
            target.insert(key, value);
        }
    }
}

fn merge_values(target: &mut toml::Value, incoming: toml::Value) {
    match (target, incoming) {
        (toml::Value::Table(target), toml::Value::Table(incoming)) => {
            merge_tables(target, incoming)
        }
        (toml::Value::Array(target), toml::Value::Array(incoming)) => target.extend(incoming),
        (target, incoming) => *target = incoming,
    }
}

fn insert_nested_value(tables: &mut toml::Table, keys: &[&str], value: toml::Value) {
    let Some((key, rest)) = keys.split_first() else {
        return;
    };
    if rest.is_empty() {
        tables.insert((*key).to_string(), value);
        return;
    }
    let table = tables
        .entry((*key).to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(table) = table {
        insert_nested_value(table, rest, value);
    }
}

pub(super) fn salvaged_critical_values(raw: &str) -> (Option<String>, Option<bool>) {
    let mut section = String::new();
    let mut token = None;
    let mut full_access = None;
    for line in raw.lines() {
        let trimmed = line.trim();
        if is_table_header(trimmed) {
            section = trimmed.to_string();
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let doc = if section.is_empty() {
            trimmed.to_string()
        } else {
            format!("{section}\n{trimmed}")
        };
        let Ok(parsed) = toml::from_str::<toml::Table>(&doc) else {
            continue;
        };
        if let Some(value) = lookup(&parsed, &["api", "token"]).and_then(toml::Value::as_str) {
            if !value.is_empty() {
                token = Some(value.to_string());
            }
        }
        if let Some(value) =
            lookup(&parsed, &["agent", "full_access"]).and_then(toml::Value::as_bool)
        {
            full_access = Some(value);
        }
    }
    (token, full_access)
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
