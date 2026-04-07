pub mod schema;

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

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
new_tab = "Ctrl+Shift+T"
close_tab = "Ctrl+Shift+W"
close_pane = "Ctrl+Shift+Q"

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

/// Load config from disk, or create default if missing.
pub fn load_config() -> AppConfig {
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Cannot determine config path: {}. Using defaults.", e);
            return AppConfig::default();
        }
    };

    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<AppConfig>(&contents) {
                Ok(mut config) => {
                    // If a named theme is specified, apply its colors as base,
                    // then overlay any user overrides.
                    resolve_theme(&mut config);
                    log::info!("Config loaded from {}", path.display());
                    config
                }
                Err(e) => {
                    log::error!("Failed to parse config at {}: {}. Using defaults.", path.display(), e);
                    AppConfig::default()
                }
            },
            Err(e) => {
                log::error!("Failed to read config at {}: {}. Using defaults.", path.display(), e);
                AppConfig::default()
            }
        }
    } else {
        // Create default config file
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("Failed to create config directory: {}", e);
                return AppConfig::default();
            }
        }
        let default_toml = default_config_toml();
        if let Err(e) = std::fs::write(&path, &default_toml) {
            log::warn!("Failed to write default config: {}", e);
        } else {
            log::info!("Default config created at {}", path.display());
        }
        AppConfig::default()
    }
}

/// Resolve theme: if config has a named theme, load its colors as a base,
/// then apply any user-specified color overrides on top.
fn resolve_theme(config: &mut AppConfig) {
    let theme_name = config.theme.name.clone();
    if let Some(base) = schema::get_any_theme(&theme_name) {
        // For each color field, if the user didn't specify it (i.e., it's the
        // default empty or matches the schema default), use the built-in theme color.
        // Since we parse the TOML, user values that are explicitly set will already
        // be in config.theme. We only fill in from the built-in if the config
        // value is the serde default (which is the github-dark default).
        // Actually, the simplest approach: the built-in theme IS the base.
        // We parse the user's [theme] section on top of it.
        // Since serde gives us the full struct with defaults already filled in,
        // we just use the built-in theme as the base when the theme name differs
        // from "github-dark" (which is already the serde default).
        if theme_name != "github-dark" {
            // Re-parse just the theme section to see what was explicitly set.
            // For simplicity and correctness, just apply the builtin theme colors.
            // Users who want to override specific colors with a non-default theme
            // will have those overrides in the TOML which will be parsed on top.
            // But since serde fills defaults from Default impl (github-dark),
            // we need a different approach: use the builtin theme as base.
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

    // Helper: if user value differs from github-dark default, it's an explicit override
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        // Defaults for fields not specified
        assert_eq!(config.terminal.scrollback, 10000);
        assert!(config.api.enabled);
        assert_eq!(config.keybindings.new_tab, "Ctrl+T");
    }

    #[test]
    fn test_resolve_theme_applies_builtin() {
        let mut config = AppConfig::default();
        config.theme.name = "dracula".to_string();
        resolve_theme(&mut config);
        // Dracula background should NOT be github-dark default
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

        let loaded: AppConfig = toml::from_str(
            &std::fs::read_to_string(&path).unwrap()
        ).unwrap();
        assert_eq!(loaded.font.size, 18);
        assert_eq!(loaded.api.port, 9999);
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
    std::fs::write(&path, toml_str)
        .map_err(|e| format!("Failed to write config: {}", e))?;
    log::info!("Config saved to {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_config(state: tauri::State<'_, ConfigState>) -> Result<AppConfig, String> {
    Ok(state.lock().clone())
}

#[tauri::command]
pub async fn update_config(
    state: tauri::State<'_, ConfigState>,
    app: AppHandle,
    config: AppConfig,
) -> Result<(), String> {
    let mut resolved = config;

    // If the theme name changed, load the builtin theme fresh instead of
    // trying to detect overrides (the frontend sends old colors with the new
    // name, so override detection breaks after the first switch).
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

    save_config(&resolved)?;
    *state.lock() = resolved.clone();
    // Emit config-changed event for live reload
    app.emit("config-changed", &resolved)
        .map_err(|e| format!("Failed to emit config-changed: {}", e))?;
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
    let dir = schema::user_themes_dir()
        .ok_or_else(|| "Cannot determine themes directory".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create themes dir: {}", e))?;
    let path = dir.join(format!("{}.toml", theme_id));
    // Validate the TOML parses as a theme before saving
    schema::parse_builtin_theme(&toml_content)?;
    std::fs::write(&path, &toml_content)
        .map_err(|e| format!("Failed to write theme: {}", e))?;
    log::info!("Custom theme saved: {} at {:?}", theme_id, path);
    Ok(())
}

/// Delete a custom theme.
#[tauri::command]
pub async fn delete_custom_theme(theme_id: String) -> Result<(), String> {
    let dir = schema::user_themes_dir()
        .ok_or_else(|| "Cannot determine themes directory".to_string())?;
    let path = dir.join(format!("{}.toml", theme_id));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete theme: {}", e))?;
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
