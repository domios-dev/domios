mod agent_access;
mod dropped_keys;
mod kpi_default;
mod load;
pub mod schema;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::AppHandle;

pub use load::load_config;
#[cfg(test)]
use load::{load_config_cached, with_generated_token, ConfigCache};
use schema::{AppConfig, ThemeConfig};

pub type ConfigState = Arc<Mutex<AppConfig>>;

fn config_path() -> Result<PathBuf, String> {
    crate::paths::config_path()
}

pub(crate) fn sanitize_plugin_settings(config: &mut AppConfig) {
    for settings in config.plugins.values_mut() {
        if let serde_json::Value::Object(fields) = settings {
            fields.remove("pinned");
        }
    }
}

/// Generate the default config TOML with helpful comments.
fn default_config_toml() -> String {
    r##"# CodeTerm Configuration
# Location: ~/.codeterm/config.toml

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
# path = "\\\\.\\pipe\\codeterm-custom"

[agent]
poll_interval_ms = 500
idle_timeout_s = 2
resume_sessions = true

[api]
enabled = true
host = "127.0.0.1"
port = 7685
# token = "your-secret-token"
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
# ~/.codeterm/kpi_ledger.db - never message text, and nothing leaves the machine.
# Set false to opt out; the Board then shows an explicit not-recording state
# instead of a figure. Takes effect on the next CodeTerm start.
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
    if api.cors_allow_origins.iter().any(|origin| {
        origin == "*"
            || !(origin.starts_with("http://") || origin.starts_with("https://"))
            || origin.parse::<axum::http::HeaderValue>().is_err()
    }) {
        return Err(
            "api.cors_allow_origins must contain explicit HTTP(S) origins, never '*'".into(),
        );
    }
    Ok(())
}

/// Resolve effective per-provider config for the given agent id.
///
/// Lookup order:
///   1. Explicit `[agents.<id>]` section in config.
///   2. Fallback to the global `[agent]` resume setting.
///   3. Built-in defaults from `ProviderConfig::default()`.
///
/// This is the single read path for provider-scoped settings — call sites
/// that previously read `cfg.agent.resume_sessions` should call this with
/// the actual provider id of the tab.
pub fn provider_config(provider_id: &str) -> schema::ProviderConfig {
    let cfg = load_config();
    let mut out = cfg.agents.get(provider_id).cloned().unwrap_or_default();
    // Back-compat: fall through to global [agent] for any field not set in the per-provider section. Lets existing configs keep working without an explicit [agents.<id>] block.
    if out.resume_sessions.is_none() {
        out.resume_sessions = Some(cfg.agent.resume_sessions);
    }
    out
}

/// Whether the usage tracker should probe this provider's `/usage` endpoint.
/// Master switch: `[usage].enabled`. Per-provider override via
/// `agents.<id>.usage_tracking_enabled`; when unset, usage-capable providers
/// default to on.
pub fn usage_tracking_enabled(provider_id: &str) -> bool {
    let cfg = load_config();
    if !cfg.usage.enabled {
        return false;
    }
    match provider_config(provider_id).usage_tracking_enabled {
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
        config.mesh.instance_name = crate::mesh::identity::instance_mesh_name_for_dev(
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
        .unwrap_or_else(|| "CodeTerm".to_string())
}

/// Resolve theme: if config has a named theme, load its colors as a base,
/// then apply any user-specified color overrides on top.
pub(crate) fn resolve_theme(config: &mut AppConfig) {
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

// Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_load_on_tokenless_config_yields_one_stable_token() {
        // The racy version generated a different token per caller, so a tab spawned with a sibling-generated token 401'd against the auth-held token.
        const N: usize = 16;
        let tmp = tempfile::tempdir().unwrap();
        let path = std::sync::Arc::new(tmp.path().join("config.toml"));
        std::fs::write(&*path, "[api]\nenabled = true\n").unwrap();

        let cache: std::sync::Arc<ConfigCache> = std::sync::Arc::new(Mutex::new(None));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(N));
        let handles: Vec<_> = (0..N)
            .map(|_| {
                let c = cache.clone();
                let p = path.clone();
                let b = barrier.clone();
                std::thread::spawn(move || {
                    b.wait();
                    load_config_cached(&p, &c).api.token
                })
            })
            .collect();
        let tokens: Vec<Option<String>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        let first = tokens[0].clone();
        assert!(
            first.as_ref().map_or(false, |t| !t.is_empty()),
            "a token must be auto-generated for a tokenless config"
        );
        for t in &tokens {
            assert_eq!(
                t, &first,
                "every concurrent caller must observe the SAME auto-generated token"
            );
        }
        let on_disk: AppConfig = toml::from_str(&std::fs::read_to_string(&*path).unwrap()).unwrap();
        assert_eq!(
            on_disk.api.token, first,
            "persisted token must match the token handed to callers"
        );
    }

    #[test]
    fn api_callback_url_normalizes_wildcard_host_to_loopback() {
        let mut api = schema::ApiConfig::default();
        api.host = "0.0.0.0".to_string();
        api.port = 7686;
        let (url, port) = api_callback_url(&api);
        assert_eq!(url, "http://127.0.0.1:7686");
        assert_eq!(port, "7686");

        api.host = "192.168.1.5".to_string();
        api.port = 7691;
        let (url, port) = api_callback_url(&api);
        assert_eq!(url, "http://192.168.1.5:7691");
        assert_eq!(port, "7691");
    }

    /// The chime answers survive the app's own read path. Reconciliation runs on
    /// every load and rebuilds the row list, so an answer dropped there would be
    /// re-defaulted on restart and the user's choice would silently expire.
    #[test]
    fn notification_level_sound_answers_survive_the_config_read() {
        use codeterm_types::notification::NotifyLevel;

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[notifications]
sound = true

[[notifications.agent_levels]]
level = "verbose"
sound = true

[[notifications.agent_levels]]
level = "action_needed"
sound = false
"#,
        )
        .unwrap();

        let cache: ConfigCache = Mutex::new(None);
        let config = load_config_cached(&path, &cache);
        let row = |level: NotifyLevel| {
            config
                .notifications
                .agent_levels
                .iter()
                .find(|candidate| candidate.level == level.as_str())
                .unwrap_or_else(|| panic!("{} has a row", level.as_str()))
        };

        assert!(config.notifications.sound, "the master mute is read back");
        assert!(
            row(NotifyLevel::Verbose).sounds(),
            "progress chimes because the file says so, against the shipped rule"
        );
        assert!(
            !row(NotifyLevel::ActionNeeded).sounds(),
            "and the level the shipped rule chimed for stays silent because the file says so"
        );
        assert!(
            row(NotifyLevel::Failure).sounds(),
            "a level the file never mentions is reconciled in with the shipped chime"
        );
        assert_eq!(
            row(NotifyLevel::Failure).sound,
            None,
            "and stays unanswered rather than being frozen to the fallback"
        );
        for stored in &config.notifications.agent_levels {
            assert!(
                !stored.channels.contains_key("sound"),
                "no sound key reached the flattened channel map: {:?}",
                stored.channels
            );
        }
    }

    #[test]
    fn test_default_config_parses() {
        let toml_str = default_config_toml();
        let config: AppConfig = toml::from_str(&toml_str).expect("default TOML should parse");
        assert_eq!(config.font.size, 14);
        assert_eq!(config.terminal.scrollback, 10000);
        assert!(config.api.enabled);
        assert_eq!(config.api.host, "127.0.0.1");
        assert!(!config.api.cors);
        assert_eq!(config.api.port, 7685);
    }

    #[test]
    fn api_loopback_spellings_have_bindable_addresses_and_valid_callback_urls() {
        for (host, bind, callback) in [
            ("localhost", "127.0.0.1:7686", "http://127.0.0.1:7686"),
            ("::1", "[::1]:7686", "http://[::1]:7686"),
            ("[::1]", "[::1]:7686", "http://[::1]:7686"),
        ] {
            let api = schema::ApiConfig {
                host: host.into(),
                port: 7686,
                ..schema::ApiConfig::default()
            };
            assert_eq!(canonical_api_bind_addr(&api).unwrap().to_string(), bind);
            assert_eq!(api_callback_url(&api).0, callback);
            assert!(validate_api_trust_boundary(&api).is_ok());
        }

        let unsupported = schema::ApiConfig {
            host: "codeterm.internal".into(),
            ..schema::ApiConfig::default()
        };
        assert!(validate_api_trust_boundary(&unsupported).is_err());
    }

    #[test]
    fn api_defaults_are_loopback_authenticated_and_cors_closed() {
        let config = with_generated_token(AppConfig::default()).0;
        assert_eq!(config.api.host, "127.0.0.1");
        assert!(!config.api.cors);
        assert!(config.api.cors_allow_origins.is_empty());
        assert!(config
            .api
            .token
            .as_ref()
            .is_some_and(|token| !token.is_empty()));
        assert!(validate_api_trust_boundary(&config.api).is_ok());

        let mut unsafe_remote = schema::ApiConfig::default();
        unsafe_remote.host = "0.0.0.0".into();
        unsafe_remote.token = None;
        assert!(validate_api_trust_boundary(&unsafe_remote).is_err());

        unsafe_remote.token = Some("explicit-network-token".into());
        assert!(validate_api_trust_boundary(&unsafe_remote).is_ok());

        let mut permissive_cors = schema::ApiConfig {
            cors: true,
            ..schema::ApiConfig::default()
        };
        assert!(validate_api_trust_boundary(&permissive_cors).is_ok());

        permissive_cors.cors_allow_origins = vec!["*".into()];
        assert!(validate_api_trust_boundary(&permissive_cors).is_err());
    }

    #[test]
    fn test_config_roundtrip() {
        let config = AppConfig::default();
        let serialized = toml::to_string_pretty(&config).expect("serialize");
        let deserialized: AppConfig = toml::from_str(&serialized).expect("deserialize");
        assert_eq!(config.theme.name, deserialized.theme.name);
        assert_eq!(config.font.family, deserialized.font.family);
        assert_eq!(config.keybindings.new_tab, deserialized.keybindings.new_tab);
        assert_eq!(config.api.port, deserialized.api.port);
    }

    #[test]
    fn test_config_partial_toml_uses_defaults() {
        let partial = r#"
[theme]
name = "dracula"

[font]
size = 16
"#;
        let config: AppConfig = toml::from_str(partial).expect("partial TOML should parse");
        assert_eq!(config.theme.name, "dracula");
        assert_eq!(config.font.size, 16);
        assert_eq!(config.terminal.scrollback, 10000);
        assert!(config.api.enabled);
        assert_eq!(config.keybindings.new_tab, "Ctrl+T");
    }

    #[test]
    fn missing_mesh_instance_name_normalizes_to_hostname() {
        let raw = r#"
[mesh]
enabled = true
"#;
        let mut config: AppConfig = toml::from_str(raw).expect("config should parse");
        assert!(config.mesh.instance_name.is_empty());

        normalize_runtime_defaults(&mut config);

        assert!(config
            .mesh
            .instance_name
            .starts_with(&default_mesh_instance_name()));
        if codeterm_paths::is_dev_instance() {
            assert!(config.mesh.instance_name.ends_with(']'));
        } else {
            assert_eq!(config.mesh.instance_name, default_mesh_instance_name());
        }
        assert!(!config.mesh.instance_name.is_empty());
    }

    #[test]
    fn test_resolve_theme_applies_builtin() {
        let mut config = AppConfig::default();
        config.theme.name = "dracula".to_string();
        resolve_theme(&mut config);
        assert_ne!(config.theme.background, "#0d1117");
        assert_eq!(config.theme.name, "dracula");
    }

    #[test]
    fn test_save_and_load_config() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        let mut config = AppConfig::default();
        config.font.size = 18;
        config.api.port = 9999;

        let toml_str = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &toml_str).unwrap();

        let loaded: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.font.size, 18);
        assert_eq!(loaded.api.port, 9999);
    }

    #[test]
    fn cleanup_removed_plugin_config_drops_plugin_and_resets_active_transcriber() {
        let mut config = AppConfig::default();
        config.plugins.insert(
            "transcriber".to_string(),
            serde_json::json!({"model": "large-v3"}),
        );
        config
            .plugins
            .insert("other".to_string(), serde_json::json!({"ok": true}));
        config.integrations.voice_transcribe.active = "transcriber".to_string();

        cleanup_removed_plugin_config_in_memory(&mut config, "transcriber");

        assert!(!config.plugins.contains_key("transcriber"));
        assert!(config.plugins.contains_key("other"));
        assert_eq!(
            config.integrations.voice_transcribe.active,
            schema::default_transcribe_active()
        );
    }

    #[test]
    fn serialized_plugin_settings_drop_legacy_pin_and_preserve_other_fields() {
        let mut config = AppConfig::default();
        config.plugins.insert(
            "bitwarden".to_string(),
            serde_json::json!({
                "pinned": true,
                "serverUrl": "https://vault.example",
                "defaultOrganization": "personal"
            }),
        );

        let serialized = serialize_config(&config).expect("config should serialize");
        let reloaded: AppConfig =
            toml::from_str(&serialized).expect("serialized config should parse");
        let settings = reloaded
            .plugins
            .get("bitwarden")
            .expect("plugin settings should remain");

        assert_eq!(settings["serverUrl"], "https://vault.example");
        assert_eq!(settings["defaultOrganization"], "personal");
        assert!(settings.get("pinned").is_none());
        assert!(config.plugins["bitwarden"].get("pinned").is_some());
    }

    /// General config invariant: a key the current schema does not recognise
    /// (a future field, or one dropped in this codebase's own history) is not
    /// silently destroyed by a load-modify-save round trip. Per-provider
    /// settings use `#[serde(flatten)] extra: HashMap<String, toml::Value>`
    /// specifically so plugin-declared keys survive a schema that predates
    /// them; this pins that the flatten bag actually round-trips.
    #[test]
    fn load_modify_save_round_trip_preserves_unrecognized_provider_keys() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            "[agents.claude]\nresume_sessions = true\nfuture_unknown_key = \"kept\"\nintegration_consent_granted = true\nintegration_consent_recorded_at = 123\nprecompact_memory_export = true\n",
        )
        .unwrap();

        let cache: ConfigCache = Mutex::new(None);
        let mut config = load_config_cached(&path, &cache);
        assert_eq!(
            config
                .agents
                .get("claude")
                .and_then(|p| p.extra.get("future_unknown_key"))
                .and_then(|v| v.as_str()),
            Some("kept"),
            "an unrecognized key must survive the initial load"
        );
        let extra = &config.agents["claude"].extra;
        assert_eq!(
            extra
                .get(crate::config::schema::INTEGRATION_CONSENT_GRANTED)
                .and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            extra
                .get("integration_consent_recorded_at")
                .and_then(toml::Value::as_integer),
            Some(123)
        );
        assert_eq!(
            extra
                .get("precompact_memory_export")
                .and_then(toml::Value::as_bool),
            Some(true)
        );

        config.font.size = 20; // unrelated modification the round trip must also keep
        let toml_str = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &toml_str).unwrap();

        let reloaded: AppConfig = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reloaded.font.size, 20);
        assert_eq!(
            reloaded
                .agents
                .get("claude")
                .and_then(|p| p.extra.get("future_unknown_key"))
                .and_then(|v| v.as_str()),
            Some("kept"),
            "the unrecognized key must still be present after save + reload"
        );
        let extra = &reloaded.agents["claude"].extra;
        assert_eq!(
            extra
                .get(crate::config::schema::INTEGRATION_CONSENT_GRANTED)
                .and_then(toml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            extra
                .get("integration_consent_recorded_at")
                .and_then(toml::Value::as_integer),
            Some(123)
        );
        assert_eq!(
            extra
                .get("precompact_memory_export")
                .and_then(toml::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn test_config_path_is_valid() {
        let path = config_path();
        assert!(path.is_ok());
        let p = path.unwrap();
        assert!(p.to_string_lossy().contains("codeterm"));
        assert!(p.to_string_lossy().contains("config.toml"));
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
    load::config_file(path)
        .save(&toml_str)
        .map_err(|e| format!("Failed to write config: {}", e))?;
    log::info!("Config saved to {}", path.display());
    Ok(())
}

pub(crate) fn save_hook_reconciliation_config(config: &AppConfig) -> Result<(), String> {
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
    if !load::is_salvaged_config(&current) {
        return Err(hold.refusal());
    }
    let contents = load::mark_salvaged_config(&serialize_config(config)?);
    file.replace_held_recovery(&contents)
        .map_err(|error| format!("Failed to write held config: {error}"))
}

fn serialize_config(config: &AppConfig) -> Result<String, String> {
    let mut sanitized = config.clone();
    sanitize_plugin_settings(&mut sanitized);
    toml::to_string_pretty(&sanitized).map_err(|e| format!("Failed to serialize config: {}", e))
}

// Tauri commands

#[tauri::command]
pub async fn get_config(state: tauri::State<'_, ConfigState>) -> Result<AppConfig, String> {
    Ok(state.lock().clone())
}

#[tauri::command]
pub async fn update_config(
    state: tauri::State<'_, ConfigState>,
    mesh_state: tauri::State<'_, crate::MeshStateHandle>,
    app: AppHandle,
    config: AppConfig,
) -> Result<(), String> {
    let mut resolved = config;
    sanitize_plugin_settings(&mut resolved);
    validate_api_trust_boundary(&resolved.api)?;

    // If the theme name changed, load the builtin theme fresh instead of trying to detect overrides.
    // The frontend sends old colors with the new name, so override detection breaks after the first switch.
    let current_name = state.lock().theme.name.clone();
    if resolved.theme.name != current_name {
        let requested_name = resolved.theme.name.clone();
        if let Some(mut builtin) = schema::get_any_theme(&requested_name) {
            builtin.name = requested_name;
            resolved.theme = builtin;
        }
    } else {
        resolve_theme(&mut resolved);
    }

    // this, paired peers stay on the old instance_name forever (the alias-propagation bug seen after an instance rename).
    let prev_name = state.lock().mesh.instance_name.clone();
    let new_name = resolved.mesh.instance_name.clone();
    let renamed = !new_name.is_empty() && new_name != prev_name;

    save_config(&resolved)?;
    *state.lock() = resolved.clone();

    if renamed {
        let mesh_arc = mesh_state.inner().clone();
        {
            let mut m = mesh_arc.lock();
            m.ident.instance_name = new_name.clone();
        }
        log::info!(
            "[mesh] instance renamed: {:?} -> {}, broadcasting announce",
            prev_name,
            new_name
        );
        tauri::async_runtime::spawn(async move {
            let report = crate::mesh::broadcast_announce(&mesh_arc, None).await;
            log::info!(
                "[mesh] post-rename announce sent={} ok={} stale={} errors={}",
                report.sent,
                report.ok,
                report.stale_peers.len(),
                report.errors.len()
            );
        });
    }

    crate::api::events::emit_everywhere(&app, "config-changed", &resolved);
    Ok(())
}

#[tauri::command]
pub async fn get_theme(state: tauri::State<'_, ConfigState>) -> Result<ThemeConfig, String> {
    Ok(state.lock().theme.clone())
}

#[tauri::command]
pub async fn list_themes() -> Result<Vec<String>, String> {
    Ok(schema::list_all_theme_ids())
}

#[tauri::command]
pub async fn get_config_path() -> Result<String, String> {
    config_path().map(|p| p.to_string_lossy().to_string())
}

/// Save a custom theme TOML file to the user themes directory.
#[tauri::command]
pub async fn save_custom_theme(theme_id: String, toml_content: String) -> Result<(), String> {
    let dir =
        schema::user_themes_dir().ok_or_else(|| "Cannot determine themes directory".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create themes dir: {}", e))?;
    let path = dir.join(format!("{}.toml", theme_id));
    schema::parse_builtin_theme(&toml_content)?;
    std::fs::write(&path, &toml_content).map_err(|e| format!("Failed to write theme: {}", e))?;
    log::info!("Custom theme saved: {} at {:?}", theme_id, path);
    Ok(())
}

/// Delete a custom theme.
#[tauri::command]
pub async fn delete_custom_theme(theme_id: String) -> Result<(), String> {
    let dir =
        schema::user_themes_dir().ok_or_else(|| "Cannot determine themes directory".to_string())?;
    let path = dir.join(format!("{}.toml", theme_id));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete theme: {}", e))?;
    }
    Ok(())
}

/// Get the user themes directory path.
#[tauri::command]
pub async fn get_themes_dir() -> Result<String, String> {
    schema::user_themes_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "Cannot determine themes directory".to_string())
}
