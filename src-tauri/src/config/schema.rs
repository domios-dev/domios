use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub theme: ThemeConfig,
    pub font: FontConfig,
    pub terminal: TerminalConfig,
    pub notifications: NotificationConfig,
    pub keybindings: KeybindingsConfig,
    pub socket: SocketConfig,
    pub agent: AgentConfig,
    pub api: ApiConfig,
    /// Whether onboarding wizard has been completed.
    #[serde(default)]
    pub onboarding_completed: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: ThemeConfig::default(),
            font: FontConfig::default(),
            terminal: TerminalConfig::default(),
            notifications: NotificationConfig::default(),
            keybindings: KeybindingsConfig::default(),
            socket: SocketConfig::default(),
            agent: AgentConfig::default(),
            api: ApiConfig::default(),
            onboarding_completed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    pub cors: bool,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "0.0.0.0".to_string(),
            port: 7685,
            token: None,
            cors: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        // GitHub Dark as default
        Self {
            name: "github-dark".to_string(),
            background: "#0d1117".to_string(),
            foreground: "#c9d1d9".to_string(),
            cursor: "#58a6ff".to_string(),
            selection: "#264f78".to_string(),
            black: "#0d1117".to_string(),
            red: "#ff7b72".to_string(),
            green: "#7ee787".to_string(),
            yellow: "#d29922".to_string(),
            blue: "#58a6ff".to_string(),
            magenta: "#bc8cff".to_string(),
            cyan: "#39d353".to_string(),
            white: "#c9d1d9".to_string(),
            bright_black: "#484f58".to_string(),
            bright_red: "#ffa198".to_string(),
            bright_green: "#56d364".to_string(),
            bright_yellow: "#e3b341".to_string(),
            bright_blue: "#79c0ff".to_string(),
            bright_magenta: "#d2a8ff".to_string(),
            bright_cyan: "#56d364".to_string(),
            bright_white: "#f0f6fc".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: u32,
    pub ligatures: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace".to_string(),
            size: 14,
            ligatures: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub scrollback: u32,
    pub copy_on_select: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            cursor_style: "block".to_string(),
            cursor_blink: true,
            scrollback: 10000,
            copy_on_select: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationConfig {
    pub desktop: bool,
    pub sound: bool,
    pub toast_duration: u32,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            desktop: true,
            sound: false,
            toast_duration: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub new_tab: String,
    pub close_tab: String,
    pub next_tab: String,
    pub prev_tab: String,
    pub split_right: String,
    pub split_down: String,
    pub close_pane: String,
    pub focus_pane_left: String,
    pub focus_pane_right: String,
    pub focus_pane_up: String,
    pub focus_pane_down: String,
    pub new_workspace: String,
    pub copy: String,
    pub paste: String,
    pub search: String,
    pub zoom_in: String,
    pub zoom_out: String,
    pub zoom_reset: String,
    pub open_browser: String,
    pub open_settings: String,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            new_tab: "Ctrl+T".to_string(),
            close_tab: "Ctrl+W".to_string(),
            next_tab: "Ctrl+Tab".to_string(),
            prev_tab: "Ctrl+Shift+Tab".to_string(),
            split_right: "Ctrl+Shift+ArrowRight".to_string(),
            split_down: "Ctrl+Shift+ArrowDown".to_string(),
            close_pane: "Ctrl+Shift+Q".to_string(),
            focus_pane_left: "Alt+ArrowLeft".to_string(),
            focus_pane_right: "Alt+ArrowRight".to_string(),
            focus_pane_up: "Alt+ArrowUp".to_string(),
            focus_pane_down: "Alt+ArrowDown".to_string(),
            new_workspace: "Ctrl+Shift+N".to_string(),
            copy: "Ctrl+Shift+C".to_string(),
            paste: "Ctrl+Shift+V".to_string(),
            search: "Ctrl+Shift+F".to_string(),
            zoom_in: "Ctrl+=".to_string(),
            zoom_out: "Ctrl+-".to_string(),
            zoom_reset: "Ctrl+0".to_string(),
            open_browser: "Ctrl+Shift+B".to_string(),
            open_settings: "Ctrl+,".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SocketConfig {
    pub path: Option<String>,
}

impl Default for SocketConfig {
    fn default() -> Self {
        Self { path: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub poll_interval_ms: u32,
    pub idle_timeout_s: u32,
    pub resume_sessions: bool,
    pub resume_skip_permissions: bool,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 500,
            idle_timeout_s: 30,
            resume_sessions: true,
            resume_skip_permissions: false,
        }
    }
}

/// Built-in theme definitions loaded at compile time.
pub struct BuiltinTheme {
    pub id: &'static str,
    pub toml_str: &'static str,
}

pub const BUILTIN_THEMES: &[BuiltinTheme] = &[
    BuiltinTheme {
        id: "github-dark",
        toml_str: include_str!("../../themes/github-dark.toml"),
    },
    BuiltinTheme {
        id: "dracula",
        toml_str: include_str!("../../themes/dracula.toml"),
    },
    BuiltinTheme {
        id: "catppuccin-mocha",
        toml_str: include_str!("../../themes/catppuccin-mocha.toml"),
    },
    BuiltinTheme {
        id: "one-dark",
        toml_str: include_str!("../../themes/one-dark.toml"),
    },
    BuiltinTheme {
        id: "nord",
        toml_str: include_str!("../../themes/nord.toml"),
    },
    BuiltinTheme {
        id: "github-light",
        toml_str: include_str!("../../themes/github-light.toml"),
    },
    BuiltinTheme {
        id: "solarized-light",
        toml_str: include_str!("../../themes/solarized-light.toml"),
    },
    BuiltinTheme {
        id: "pip-boy",
        toml_str: include_str!("../../themes/pip-boy.toml"),
    },
];

/// Parse a built-in theme TOML string into a ThemeConfig.
pub fn parse_builtin_theme(toml_str: &str) -> Result<ThemeConfig, String> {
    toml::from_str(toml_str).map_err(|e| format!("Failed to parse theme: {}", e))
}

/// Get a built-in theme by name. Returns None if not found.
pub fn get_builtin_theme(name: &str) -> Option<ThemeConfig> {
    BUILTIN_THEMES
        .iter()
        .find(|t| t.id == name)
        .and_then(|t| parse_builtin_theme(t.toml_str).ok())
}

/// List all available built-in theme IDs.
pub fn list_builtin_theme_ids() -> Vec<String> {
    BUILTIN_THEMES.iter().map(|t| t.id.to_string()).collect()
}

/// Directory for user custom themes.
pub fn user_themes_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("codeterm").join("themes"))
}

/// Load a custom theme from the user themes directory.
pub fn get_custom_theme(name: &str) -> Option<ThemeConfig> {
    let dir = user_themes_dir()?;
    let path = dir.join(format!("{}.toml", name));
    if path.exists() {
        let content = std::fs::read_to_string(&path).ok()?;
        parse_builtin_theme(&content).ok()
    } else {
        None
    }
}

/// List all custom theme IDs from the user themes directory.
pub fn list_custom_theme_ids() -> Vec<String> {
    let Some(dir) = user_themes_dir() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".toml").map(|s| s.to_string())
        })
        .collect()
}

/// Get a theme by name — checks builtins first, then user custom themes.
pub fn get_any_theme(name: &str) -> Option<ThemeConfig> {
    get_builtin_theme(name).or_else(|| get_custom_theme(name))
}

/// List all theme IDs (builtins + custom).
pub fn list_all_theme_ids() -> Vec<String> {
    let mut ids = list_builtin_theme_ids();
    ids.extend(list_custom_theme_ids());
    ids
}
