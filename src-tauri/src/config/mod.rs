pub mod schema;

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::AppHandle;

use schema::{AppConfig, ThemeConfig};

pub type ConfigState = Arc<Mutex<AppConfig>>;

fn config_path() -> Result<PathBuf, String> {
    crate::paths::config_path()
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

[notifications]
desktop = true
sound = false
toast_duration = 5  # seconds

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
resume_skip_permissions = true

[api]
enabled = false
port = 7685
# token = "your-secret-token"
cors = true
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
    let host = if api.host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        api.host.as_str()
    };
    (
        format!("http://{}:{}", host, api.port),
        api.port.to_string(),
    )
}

/// Resolve effective per-provider config for the given agent id.
///
/// Lookup order:
///   1. Explicit `[agents.<id>]` section in config.
///   2. Back-compat fallback to the global `[agent]` section
///      (`resume_sessions`, `resume_skip_permissions`).
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
    if out.resume_skip_permissions.is_none() {
        out.resume_skip_permissions = Some(cfg.agent.resume_skip_permissions);
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

/// Load config from disk, or create default if missing.
/// Auto-generates an API token when none is configured — the internal
/// webview and the CLI both need it to authenticate against the REST API.
/// Cached config read. `config.toml` is re-parsed only when its mtime changes.
/// `load_config()` is called from many hot paths (mesh heartbeats, request
/// handlers, bridge/secrets/voice) — a disk read + full TOML parse per call was
/// a measurable CPU sink (profiled). `config set` rewrites the file so the mtime
/// bumps and readers see the new value on the next call.
type ConfigCache = Mutex<Option<(Option<std::time::SystemTime>, AppConfig)>>;

pub fn load_config() -> AppConfig {
    use std::sync::OnceLock;
    static CACHE: OnceLock<ConfigCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    match config_path() {
        Ok(path) => load_config_cached(&path, cache),
        Err(e) => {
            log::warn!("Cannot determine config path: {}. Using defaults.", e);
            ensure_api_token(AppConfig::default(), None)
        }
    }
}

/// Cache the parsed config, keyed by file mtime. The cache lock is held across
/// the uncached fill so a token auto-generated for a tokenless config is
/// produced EXACTLY once: concurrent callers can no longer each generate a
/// different token (the auth-token divergence that 401'd one tab while another
/// tab with a sibling-generated token succeeded). The mtime is re-read AFTER
/// the fill because `ensure_api_token` may rewrite the file — caching the
/// post-write mtime keeps the next call a hit instead of a perpetual miss.
fn load_config_cached(path: &std::path::Path, cache: &ConfigCache) -> AppConfig {
    let mut guard = cache.lock();
    let mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    if let Some((cached_mtime, cfg)) = guard.as_ref() {
        if *cached_mtime == mtime {
            return cfg.clone();
        }
    }
    match load_config_uncached_at(path) {
        Ok(cfg) => {
            let post_mtime = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
            *guard = Some((post_mtime, cfg.clone()));
            cfg
        }
        // Transient read/parse failure: keep the last-known good config instead
        // of caching a token-less default that would 401 every tab.
        Err(()) => match guard.as_ref() {
            Some((_, cfg)) => cfg.clone(),
            None => ensure_api_token(AppConfig::default(), None),
        },
    }
}

/// `Err` means a TRANSIENT failure (unreadable/unparseable file) — the caller
/// must keep any previously cached config rather than fall back to a token-less
/// default, since a token-less config reaching the auth layer 401s every tab.
fn load_config_uncached_at(path: &std::path::Path) -> Result<AppConfig, ()> {
    if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(contents) => match toml::from_str::<AppConfig>(&contents) {
                Ok(mut config) => {
                    resolve_theme(&mut config);
                    normalize_runtime_defaults(&mut config);
                    // Mesh tab disabled → force offline. Tab is the on/off switch for the whole subsystem; flag is the runtime presence state.
                    if !config.ui.mesh_tab_enabled {
                        config.mesh.enabled = false;
                    }
                    log::info!("Config loaded from {}", path.display());
                    Ok(ensure_api_token(config, Some(path)))
                }
                Err(e) => {
                    log::error!(
                        "Failed to parse config at {}: {} — keeping last-known config",
                        path.display(),
                        e
                    );
                    Err(())
                }
            },
            Err(e) => {
                log::error!(
                    "Failed to read config at {}: {} — keeping last-known config",
                    path.display(),
                    e
                );
                Err(())
            }
        }
    } else {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("Failed to create config directory: {}", e);
                return Ok(ensure_api_token(AppConfig::default(), None));
            }
        }
        let config = ensure_api_token(AppConfig::default(), None);
        let toml_str = toml::to_string_pretty(&config).unwrap_or_else(|_| default_config_toml());
        if let Err(e) = write_config_atomic(path, &toml_str) {
            log::warn!("Failed to write default config: {}", e);
        } else {
            log::info!("Default config created at {}", path.display());
        }
        Ok(config)
    }
}

/// Crash-safe config write: serialize to a sibling temp file, then rename over
/// the target. A concurrent reader sees either the old or the new file whole,
/// never a truncated `fs::write` in progress (a torn read parses as a token-less
/// config and 401s the tab that read it). `rename` replaces the destination
/// atomically on both Unix and Windows.
fn write_config_atomic(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("toml.tmp.{}.{}", std::process::id(), seq));
    std::fs::write(&tmp, contents)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn ensure_api_token(mut config: AppConfig, path: Option<&std::path::Path>) -> AppConfig {
    normalize_runtime_defaults(&mut config);
    if config.api.token.as_ref().map_or(true, |t| t.is_empty()) {
        let token = uuid::Uuid::new_v4().to_string().replace("-", "");
        log::info!("[config] auto-generated API token (none was configured)");
        config.api.token = Some(token);
        if let Some(p) = path {
            let toml_str = toml::to_string_pretty(&config).unwrap_or_default();
            let _ = write_config_atomic(p, &toml_str);
        }
    }
    config
}

fn normalize_runtime_defaults(config: &mut AppConfig) {
    if config.mesh.instance_name.trim().is_empty() {
        config.mesh.instance_name = default_mesh_instance_name();
    }
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
        // Repro for the intermittent multi-agent-boot 401: a tokenless config
        // hit by N concurrent load_config() callers must auto-generate EXACTLY
        // one token that every caller (and the persisted file) agrees on. The
        // racy version generated a different token per caller, so a tab spawned
        // with a sibling-generated token 401'd against the auth-held token.
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

    #[test]
    fn test_default_config_parses() {
        let toml_str = default_config_toml();
        let config: AppConfig = toml::from_str(&toml_str).expect("default TOML should parse");
        assert_eq!(config.font.size, 14);
        assert_eq!(config.terminal.scrollback, 10000);
        assert_eq!(config.api.port, 7685);
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

        assert_eq!(config.mesh.instance_name, default_mesh_instance_name());
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    let toml_str =
        toml::to_string_pretty(config).map_err(|e| format!("Failed to serialize config: {}", e))?;
    write_config_atomic(&path, &toml_str).map_err(|e| format!("Failed to write config: {}", e))?;
    log::info!("Config saved to {}", path.display());
    Ok(())
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
