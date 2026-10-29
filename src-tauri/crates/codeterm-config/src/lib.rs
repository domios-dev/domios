pub mod bounds;
mod load;
pub mod schema;

use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

pub use load::load_config;
#[cfg(test)]
use load::{load_config_cached, ConfigCache};
use schema::{AppConfig, ThemeConfig};

pub type ConfigState = Arc<Mutex<AppConfig>>;

fn config_path() -> Result<PathBuf, String> {
    codeterm_paths::config_path()
}

pub fn sanitize_plugin_settings(config: &mut AppConfig) {
    for settings in config.plugins.values_mut() {
        if let serde_json::Value::Object(fields) = settings {
            fields.remove("pinned");
        }
    }
}

pub fn changed_provider_settings(before: &AppConfig, after: &AppConfig) -> Vec<String> {
    let mut changed = BTreeSet::new();
    changed.extend(changed_settings_keys(&before.plugins, &after.plugins));
    changed.extend(changed_settings_keys(&before.agents, &after.agents));
    changed.into_iter().collect()
}

fn changed_settings_keys<T: serde::Serialize>(
    before: &HashMap<String, T>,
    after: &HashMap<String, T>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|id| {
            serde_json::to_value(before.get(*id)).ok() != serde_json::to_value(after.get(*id)).ok()
        })
        .cloned()
        .collect()
}

/// Generate the default config TOML with helpful comments.
fn default_config_toml() -> String {
    r##"# Domios Configuration
# Location: ~/.domios/config.toml

[theme]
name = "github-dark"
# Available themes: github-dark, dracula, catppuccin-mocha, one-dark, nord
# Override individual colors:
# background = "#0d1117"
# foreground = "#c9d1d9"
# cursor = "#58a6ff"
# selection = "#264f78"

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"  # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

[diagnostics]
terminal_transport = false

[notifications]
desktop = false  # opt-in; the per-level grid below still decides which levels toast
sound = false  # the chime's master mute; each level below says whether it chimes
toast_duration = 5  # seconds
show_bell = true  # false hides the indicator; every record is still kept
# Where each level may go BEYOND the panel, which keeps every record whatever
# is written here. Rows are generated from the shared level set, so a level
# added later appears here on its own. A fresh install interrupts nowhere, at
# every level. Any combination is legal, including none. A column nobody named
# is off, so opting a level into a destination means writing that column out. A
# notification naming no level is kept too, and never reaches the system or a
# channel. `bell` is retained and inert: nothing routes on it, and it stays a
# named field so the flattened channel keys never read it back as a channel.
# `sound` is a named field too, and IS chosen per level: write it to say whether
# that level chimes. Omit it and the level keeps the chime that shipped -
# action_needed and failure chime, verbose does not. `notifications.sound` above
# is the master mute over all of them.
# [[notifications.agent_levels]]
# level = "action_needed"
# desktop = true
# sound = false
# telegram = true

[keybindings]
new_tab = "Ctrl+T"
reopen_tab = "Ctrl+Shift+T"
close_tab = "Ctrl+Shift+W"

[socket]
# Socket path is auto-generated, override if needed:
# path = "\\\\.\\pipe\\domios-custom"

[agent]
poll_interval_ms = 500
idle_timeout_s = 2
resume_sessions = true
continue_after_restart = true

[api]
enabled = true
host = "127.0.0.1"
port = 7685
# Instance credentials are stored in .secrets, separately from this file.
cors = false
# cors_allow_origins = ["https://trusted.example"]

[orchestration.map]
# The workspace essence map. Off until you opt in: with it on, a plan wave
# closing or a release boundary offers one Cartographer run that regenerates
# essences/map.jsonl from the source essences and logs its decision. A
# repository without those source essences is never map-enabled.
enabled = false

[kpi]
# Local orchestration-economics ledger behind Mission Control's token and turn
# figures. It stores plan/task/tab ids, counts and timestamps in
# ~/.domios/kpi_ledger.db - never message text, and nothing leaves the machine.
# Set false to opt out; the Board then shows an explicit not-recording state
# instead of a figure. Takes effect on the next Domios start.
enabled = true
"##
    .to_string()
}

/// Default agent provider id when caller has no explicit choice.
/// Single source of truth — read by all sites that previously hardcoded `"claude"`.
/// Sourced from `[general_agent].agent_provider` in config (defaults to `"claude"`).
pub fn default_agent_provider() -> String {
    load_config().general_agent.agent_provider.clone()
}

/// Memory config accessor.
pub fn memory_config() -> schema::MemoryConfig {
    load_config().memory.clone()
}

/// Loopback callback URL + port string for the running API, with a `0.0.0.0`
/// bind normalized to `127.0.0.1` so in-process hooks/reports reach the local
/// listener. Single source so tab-env, the frontend PTY path, hidden spawns,
/// and the startup env scrub cannot derive divergent targets — a tab built
/// with a prod-inherited URL (7685) 401s this instance's token against prod.
pub fn api_callback_url(api: &schema::ApiConfig) -> (String, String) {
    let bind = canonical_api_bind_addr(api)
        .unwrap_or_else(|_| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), api.port));
    let callback_ip = match bind.ip() {
        IpAddr::V4(address) if address.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(address) if address.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        address => address,
    };
    let host = match callback_ip {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    (format!("http://{host}:{}", api.port), api.port.to_string())
}

pub fn canonical_api_bind_addr(api: &schema::ApiConfig) -> Result<SocketAddr, String> {
    codeterm_paths::validate_listener_port(api.port, cfg!(debug_assertions))?;
    let trimmed = api.host.trim();
    let host = if trimmed.eq_ignore_ascii_case("localhost") {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        let unbracketed = trimmed
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .unwrap_or(trimmed);
        unbracketed.parse::<IpAddr>().map_err(|_| {
            format!(
                "api.host '{}' is unsupported; use localhost or a numeric IPv4/IPv6 address",
                api.host
            )
        })?
    };
    Ok(SocketAddr::new(host, api.port))
}

pub fn exact_cors_origin(origin: &str) -> bool {
    if origin.contains('*') {
        return false;
    }
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.origin().ascii_serialization() == origin
}

pub fn validate_api_trust_boundary(api: &schema::ApiConfig) -> Result<(), String> {
    let bind = canonical_api_bind_addr(api)?;
    let has_token = api
        .token
        .as_ref()
        .is_some_and(|token| !token.trim().is_empty());
    if !bind.ip().is_loopback() && !has_token {
        return Err(format!(
            "non-loopback API host '{}' requires an explicit bearer token",
            api.host
        ));
    }
    if let Some((index, _)) = api
        .cors_allow_origins
        .iter()
        .enumerate()
        .find(|(_, origin)| !exact_cors_origin(origin))
    {
        log::warn!("[rest-cors] refused api.cors_allow_origins entry={index}: expected an exact HTTP(S) origin");
        return Err("api.cors_allow_origins must contain exact HTTP(S) origins, never '*'".into());
    }
    Ok(())
}

pub fn provider_config_from(
    config: &schema::AppConfig,
    provider_id: &str,
) -> schema::ProviderConfig {
    let mut out = config.agents.get(provider_id).cloned().unwrap_or_default();
    // Back-compat: fall through to global [agent] for any field not set in the per-provider section. Lets existing configs keep working without an explicit [agents.<id>] block.
    if out.resume_sessions.is_none() {
        out.resume_sessions = Some(config.agent.resume_sessions);
    }
    out
}

pub fn usage_tracking_enabled_from(cfg: &schema::AppConfig, provider_id: &str) -> bool {
    if !cfg.usage.enabled {
        return false;
    }
    match provider_config_from(cfg, provider_id).usage_tracking_enabled {
        Some(false) => false,
        Some(true) | None => true,
    }
}

pub fn cleanup_removed_plugin_config(plugin_id: &str) -> Result<(), String> {
    let mut config = load_config();
    cleanup_removed_plugin_config_in_memory(&mut config, plugin_id);
    save_config(&config)
}

fn cleanup_removed_plugin_config_in_memory(config: &mut AppConfig, plugin_id: &str) {
    config.plugins.remove(plugin_id);
    if config.integrations.voice_transcribe.active == plugin_id {
        config.integrations.voice_transcribe.active = schema::default_transcribe_active();
    }
}

fn normalize_runtime_defaults(config: &mut AppConfig) {
    if config.mesh.instance_name.trim().is_empty() {
        config.mesh.instance_name = default_mesh_instance_name();
    }
    if codeterm_paths::is_dev_instance() {
        config.mesh.instance_name = codeterm_mesh::identity::instance_mesh_name_for_dev(
            &config.mesh.instance_name,
            &codeterm_paths::instance_suffix(),
        );
    }
    // The grid is the shared level set mapped. Reconciling on read is what lets
    // a level join the set without a migration and without the panel drifting.
    config.notifications.agent_levels =
        schema::reconcile_agent_levels(&config.notifications.agent_levels);
}

fn default_mesh_instance_name() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().trim_end_matches(".local").to_string())
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Domios".to_string())
}

/// Resolve theme: if config has a named theme, load its colors as a base,
/// then apply any user-specified color overrides on top.
pub fn resolve_theme(config: &mut AppConfig) {
    let theme_name = config.theme.name.clone();
    if let Some(base) = schema::get_any_theme(&theme_name) {
        // For each color field, if the user didn't set it (empty or serde default), use the built-in theme color.
        // Config starts from built-in defaults and layers parsed [theme] values on top; this preserves explicit overrides while keeping unset fields themed.
        if theme_name != "github-dark" {
            // Re-parse just the theme section to detect explicit fields; apply built-in colors as fallback.
            // Serde fills defaults from github-dark, so this avoids treating those defaults as intentional user values.
            config.theme = apply_theme_with_overrides(&base, &config.theme, &theme_name);
        }
    }
}

/// Apply a built-in theme as the base and overlay any explicit user overrides.
/// We detect "overrides" by checking if a color differs from the github-dark default.
fn apply_theme_with_overrides(
    builtin: &ThemeConfig,
    user: &ThemeConfig,
    _theme_name: &str,
) -> ThemeConfig {
    let gh_default = ThemeConfig::default();

    macro_rules! pick {
        ($field:ident) => {
            if user.$field != gh_default.$field {
                user.$field.clone()
            } else {
                builtin.$field.clone()
            }
        };
    }

    ThemeConfig {
        name: user.name.clone(),
        background: pick!(background),
        foreground: pick!(foreground),
        cursor: pick!(cursor),
        selection: pick!(selection),
        black: pick!(black),
        red: pick!(red),
        green: pick!(green),
        yellow: pick!(yellow),
        blue: pick!(blue),
        magenta: pick!(magenta),
        cyan: pick!(cyan),
        white: pick!(white),
        bright_black: pick!(bright_black),
        bright_red: pick!(bright_red),
        bright_green: pick!(bright_green),
        bright_yellow: pick!(bright_yellow),
        bright_blue: pick!(bright_blue),
        bright_magenta: pick!(bright_magenta),
        bright_cyan: pick!(bright_cyan),
        bright_white: pick!(bright_white),
    }
}

/// Save the current config to disk.
pub fn save_config_pub(config: &AppConfig) -> Result<(), String> {
    save_config(config)
}

fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    save_config_at(&path, config)
}

fn save_config_at(path: &std::path::Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    let toml_str = serialize_config(config)?;
    let file = load::config_file(path);
    file.save(&toml_str).map_err(|error| match file.held() {
        Some(hold) => format!(
            concat!(
                "Config is held: {}. Review or repair the salvaged config, remove its ",
                "Domios salvage marker, or restore a known-good config backup. Restart ",
                "Domios to reload it and release the hold."
            ),
            hold.refusal()
        ),
        None => format!("Failed to write config: {error}"),
    })?;
    log::info!("Config saved to {}", path.display());
    Ok(())
}

fn save_config_and_apply_at(
    path: &std::path::Path,
    state: &Mutex<AppConfig>,
    mut config: AppConfig,
) -> Result<(), String> {
    config.api.token = state.lock().api.token.clone();
    save_config_at(path, &config)?;
    *state.lock() = config;
    Ok(())
}

pub fn save_config_and_apply(state: &ConfigState, config: AppConfig) -> Result<(), String> {
    let path = config_path()?;
    save_config_and_apply_at(&path, state, config)
}

pub fn save_hook_reconciliation_config(config: &AppConfig) -> Result<(), String> {
    let path = config_path()?;
    save_hook_reconciliation_config_at(&path, config)
}

fn save_hook_reconciliation_config_at(
    path: &std::path::Path,
    config: &AppConfig,
) -> Result<(), String> {
    let file = load::config_file(path);
    let Some(hold) = file.held() else {
        return save_config_at(path, config);
    };
    let current = std::fs::read_to_string(path)
        .map_err(|error| format!("Failed to read held config: {error}"))?;
    if !load::is_salvaged_config(&current) || load::salvaged_critical_values(&current).0.is_some() {
        return Err(hold.refusal());
    }
    let contents = load::mark_salvaged_config(&serialize_config(config)?);
    file.replace_held_recovery(&contents)
        .map_err(|error| format!("Failed to write held config: {error}"))
}

fn serialize_config(config: &AppConfig) -> Result<String, String> {
    let mut sanitized = config.clone();
    sanitize_plugin_settings(&mut sanitized);
    bounds::validate_app_config(&sanitized)?;
    toml::to_string_pretty(&sanitized).map_err(|e| format!("Failed to serialize config: {}", e))
}

#[cfg(test)]
#[path = "lib/tests.rs"]
mod tests;
