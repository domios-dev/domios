use std::path::Path;

use parking_lot::Mutex;

use super::schema::AppConfig;
use super::{normalize_runtime_defaults, resolve_theme};
use codeterm_util::persisted_state::{Loaded, StateFile};

pub(super) const SALVAGED_CONFIG_MARKER: &str =
    "# Domios salvaged config; remove this marker after reviewing or restoring the corrupt copy.";

// Reload settings on file changes while retaining the startup credential snapshot.
pub(super) type ConfigCache = Mutex<Option<(Option<std::time::SystemTime>, AppConfig)>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenPolicy {
    GenerateWhenMissing,
    KeepConfigured,
}

#[cfg(test)]
pub fn load_config() -> AppConfig {
    panic!("Ambient user config is forbidden in unit tests; inject ConfigState or explicit config")
}

#[cfg(not(test))]
pub fn load_config() -> AppConfig {
    use std::sync::OnceLock;
    static CACHE: OnceLock<ConfigCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let result = super::config_path()
        .and_then(|path| load_config_cached_at(&path, &codeterm_paths::data_dir_path(), cache));
    result.unwrap_or_else(|error| {
        log::error!("[config] Domios startup refused: {error}");
        eprintln!("Domios startup refused: {error}");
        std::process::exit(2);
    })
}

pub(super) fn config_file(path: &Path) -> StateFile {
    StateFile::new(path)
        .with_private_permissions()
        .with_validator(validate_config)
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

#[cfg(test)]
pub(super) fn load_config_cached(path: &Path, cache: &ConfigCache) -> AppConfig {
    load_config_cached_at(path, path.parent().unwrap(), cache).unwrap()
}

fn permission_warning(path: &Path, result: std::io::Result<()>) -> Option<String> {
    result.err().map(|error| {
        format!(
            "Cannot tighten permissions on {}: {error}; continuing",
            path.display()
        )
    })
}

fn tighten_permissions(path: &Path) {
    if let Some(warning) =
        permission_warning(path, codeterm_util::private_file::restrict_if_present(path))
    {
        log::warn!("[config] {warning}");
    }
}

fn credential_failure(token: Option<&str>, error: String) -> Result<(), String> {
    if token.is_some_and(|token| !token.trim().is_empty()) {
        Ok(())
    } else {
        Err(error)
    }
}

fn load_config_cached_at(
    path: &Path,
    data_root: &Path,
    cache: &ConfigCache,
) -> Result<AppConfig, String> {
    let mut guard = cache.lock();
    let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    if let Some((cached_mtime, cfg)) = guard.as_ref() {
        if *cached_mtime == mtime {
            return Ok(cfg.clone());
        }
    }
    let last_known = guard.as_ref().map(|(_, cfg)| cfg.clone());
    if last_known.is_none() {
        tighten_permissions(&data_root.join(".secrets"));
    }
    tighten_permissions(path);
    let cfg = load_config_uncached_at(path, data_root, last_known)?;
    // Re-read after the fill: a generated token or a restored backup rewrites the file.
    let post_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    *guard = Some((post_mtime, cfg.clone()));
    Ok(cfg)
}

fn load_config_uncached_at(
    path: &Path,
    data_root: &Path,
    last_known: Option<AppConfig>,
) -> Result<AppConfig, String> {
    let file = config_file(path);
    if std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
        file.hold_after_error("Config path must resolve to a regular file; writes are held");
        return held_config(&file, data_root, last_known, None);
    }
    let cached_token = last_known.as_ref().map(|config| config.api.token.clone());
    if let Ok(contents) = std::fs::read_to_string(path) {
        if is_salvaged_config(&contents) {
            file.hold_for_repair();
            if let Ok((config, contents)) = parse_config(&contents) {
                return finish_loaded(
                    &file,
                    data_root,
                    config,
                    &contents,
                    TokenPolicy::KeepConfigured,
                    cached_token.as_ref(),
                );
            }
        }
    }
    match file.load(parse_config) {
        Loaded::Parsed((config, contents)) => finish_loaded(
            &file,
            data_root,
            config,
            &contents,
            TokenPolicy::GenerateWhenMissing,
            cached_token.as_ref(),
        ),
        Loaded::Recovered {
            value: (config, contents),
            ..
        } => finish_loaded(
            &file,
            data_root,
            config,
            &contents,
            TokenPolicy::KeepConfigured,
            cached_token.as_ref(),
        ),
        Loaded::Unrecoverable { raw } => held_config(&file, data_root, last_known, raw.as_deref()),
        Loaded::Missing if file.held().is_some() => held_config(&file, data_root, last_known, None),
        Loaded::Missing => create_default(&file, data_root, cached_token.as_ref()),
    }
}

fn finish_loaded(
    file: &StateFile,
    data_root: &Path,
    mut config: AppConfig,
    contents: &str,
    tokens: TokenPolicy,
    cached_token: Option<&Option<String>>,
) -> Result<AppConfig, String> {
    resolve_theme(&mut config);
    normalize_runtime_defaults(&mut config);
    // Mesh tab disabled → force offline. Tab is the on/off switch for the whole subsystem; flag is the runtime presence state.
    if !config.ui.mesh_tab_enabled {
        config.mesh.enabled = false;
    }
    log::info!("Config loaded from {}", file.path().display());
    let migrated_token =
        match apply_instance_token(data_root, &mut config, contents, tokens, cached_token) {
            Ok(migrated) => migrated,
            Err(error) => {
                file.hold_after_error(&error);
                credential_failure(config.api.token.as_deref(), error)?;
                return Ok(config);
            }
        };
    if migrated_token {
        persist_migrated_config(file, &config, |contents| {
            if file.held().is_some() {
                file.replace_held_recovery(&mark_salvaged_config(contents))
            } else {
                file.save(contents)
            }
        });
    }
    Ok(config)
}

fn persist_migrated_config(
    file: &StateFile,
    config: &AppConfig,
    write: impl FnOnce(&str) -> Result<(), String>,
) {
    let result = toml::to_string_pretty(config)
        .map_err(|error| error.to_string())
        .and_then(|contents| write(&contents));
    if let Err(error) = result {
        file.hold_after_error(&format!(
            "Cannot persist migrated config: {error}; runtime credential retained and writes held"
        ));
    }
}

fn create_default(
    file: &StateFile,
    data_root: &Path,
    cached_token: Option<&Option<String>>,
) -> Result<AppConfig, String> {
    let mut config = AppConfig::default();
    normalize_runtime_defaults(&mut config);
    apply_instance_token(
        data_root,
        &mut config,
        "",
        TokenPolicy::GenerateWhenMissing,
        cached_token,
    )?;
    let toml_str = toml::to_string_pretty(&config).unwrap_or_else(|_| super::default_config_toml());
    match file.save(&toml_str) {
        Ok(()) => log::info!("Default config created at {}", file.path().display()),
        Err(error) => file.hold_after_error(&format!(
            "Cannot persist default config: {error}; runtime credential retained and writes held"
        )),
    }
    Ok(config)
}

/// Recover intact settings without generating authority when none can be recovered.
fn held_config(
    file: &StateFile,
    data_root: &Path,
    last_known: Option<AppConfig>,
    raw: Option<&str>,
) -> Result<AppConfig, String> {
    let cached_token = last_known.as_ref().map(|config| config.api.token.clone());
    let (mut config, contents) = match (last_known, raw) {
        (Some(config), raw) => {
            let contents = toml::to_string_pretty(&config).ok().map(|contents| {
                let legacy = raw.and_then(|raw| salvaged_critical_values(raw).0);
                if let Some(token) = legacy.filter(|token| config.api.token.as_ref() == Some(token))
                {
                    let mut tables = contents.parse::<toml::Table>().unwrap_or_default();
                    insert_nested_value(&mut tables, &["api", "token"], toml::Value::String(token));
                    return mark_salvaged_config(&toml::to_string_pretty(&tables).unwrap());
                }
                mark_salvaged_config(&contents)
            });
            (config, contents)
        }
        (None, Some(raw)) => {
            let (config, contents) = salvage_config(raw)
                .unwrap_or_else(|| (AppConfig::default(), mark_salvaged_config("")));
            (config, Some(contents))
        }
        (None, None) => (AppConfig::default(), None),
    };
    let migration = apply_instance_token(
        data_root,
        &mut config,
        contents.as_deref().unwrap_or(""),
        TokenPolicy::KeepConfigured,
        cached_token.as_ref(),
    );
    if let Err(error) = &migration {
        file.hold_after_error(error);
        credential_failure(config.api.token.as_deref(), error.clone())?;
    }
    if let Some(contents) = contents {
        let contents = if migration.is_ok() {
            remove_legacy_token(&contents)
        } else {
            contents
        };
        if let Err(error) = file.create_held_recovery(&contents) {
            log::warn!("[config] could not write salvaged config: {error}");
        }
    }
    normalize_runtime_defaults(&mut config);
    if config.api.token.as_ref().map_or(true, |t| t.is_empty()) {
        log::warn!(
            "[config] no API credential could be recovered; token-authenticated calls are refused"
        );
    }
    Ok(config)
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

fn apply_instance_token(
    data_root: &Path,
    config: &mut AppConfig,
    contents: &str,
    policy: TokenPolicy,
    cached_token: Option<&Option<String>>,
) -> Result<bool, String> {
    let raw = toml::from_str::<toml::Value>(contents).ok();
    let legacy = raw
        .as_ref()
        .and_then(|value| value.get("api"))
        .and_then(|api| api.get("token"));
    let legacy_token = legacy
        .and_then(toml::Value::as_str)
        .filter(|token| !token.trim().is_empty());
    config.api.token = match (cached_token, legacy_token) {
        (Some(token), _) => token.clone(),
        (None, Some(legacy_token)) => {
            match codeterm_util::instance_token::migrate_at(data_root, legacy_token) {
                Ok(token) => Some(token),
                Err(error) => {
                    config.api.token = Some(legacy_token.to_owned());
                    return Err(format!("Instance API credential rotation failed: {error}; the legacy credential remains active and config writes are held until restart"));
                }
            }
        }
        (None, None) => codeterm_util::instance_token::initialize_at(
            data_root,
            None,
            policy == TokenPolicy::GenerateWhenMissing,
        )
        .map_err(|error| {
            format!(
                "Cannot initialize instance API credential at {}: {error}",
                data_root.join(".secrets").display()
            )
        })?,
    };
    if legacy_token.is_some() && config.api.token.as_deref() == legacy_token {
        return Err("Instance API credential rotation is pending; the legacy credential remains active and config writes are held until restart".into());
    }
    Ok(legacy.is_some())
}

fn remove_legacy_token(contents: &str) -> String {
    let Ok(mut value) = contents.parse::<toml::Table>() else {
        return contents.to_owned();
    };
    if let Some(api) = value.get_mut("api").and_then(toml::Value::as_table_mut) {
        api.remove("token");
    }
    let body = toml::to_string_pretty(&value)
        .unwrap_or_else(|error| panic!("Cannot serialize recovered config: {error}"));
    if is_salvaged_config(contents) {
        mark_salvaged_config(&body)
    } else {
        body
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod isolation_tests;
