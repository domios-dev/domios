use std::collections::HashMap;

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
    /// Per-provider settings keyed by provider id.
    /// Section header in TOML: `[agents.claude]`, `[agents.codex]`, etc.
    /// Plugin authors declare what fields belong here via the
    /// `settings_schema()` trait method; the renderer reads/writes
    /// the `agents.<id>.*` paths.
    #[serde(default)]
    pub agents: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub general_agent: GeneralAgentConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub mesh: MeshConfig,
    #[serde(default)]
    pub updater: UpdaterConfig,
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
            agents: HashMap::new(),
            general_agent: GeneralAgentConfig::default(),
            memory: MemoryConfig::default(),
            integrations: IntegrationsConfig::default(),
            mesh: MeshConfig::default(),
            updater: UpdaterConfig::default(),
            onboarding_completed: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdaterConfig {
    pub channel: crate::update::Channel,
}

/// Per-provider settings owned by an agent plugin.
/// Plugins declare which of these fields they use through `settings_schema()`.
/// All fields are optional so a brand-new plugin can add a `[agents.<id>]`
/// section without colliding with unknown keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    /// Resume previous sessions on CodeTerm restart (per provider).
    pub resume_sessions: Option<bool>,
    /// Use the provider's auto-approve flag when resuming
    /// (Claude: --dangerously-skip-permissions, Codex: --dangerously-bypass-...).
    pub resume_skip_permissions: Option<bool>,
    /// Default model id used for fresh sessions.
    pub default_model: Option<String>,
    /// Free-form per-provider settings the schema can declare without
    /// requiring a Rust struct field. Pluggable.
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
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
            enabled: true,
            host: "0.0.0.0".to_string(),
            #[cfg(debug_assertions)]
            port: 7686,
            #[cfg(not(debug_assertions))]
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
            idle_timeout_s: 2,
            resume_sessions: true,
            resume_skip_permissions: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralAgentConfig {
    pub enabled: bool,
    pub agent_provider: String,
    pub auto_restart: bool,
    pub max_restart_attempts: u32,
    pub restart_delay_s: u32,
}

impl Default for GeneralAgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_provider: "claude".to_string(),
            auto_restart: true,
            max_restart_attempts: 5,
            restart_delay_s: 5,
        }
    }
}

fn default_worker_model() -> String {
    "claude-haiku-4-5-20251001".to_string()
}

fn default_worker_timeout_seconds() -> u64 {
    150
}

fn default_dedup_threshold() -> f64 {
    0.15
}

fn default_file_sync_interval() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Agent provider for extraction workers (default: same as general_agent)
    pub extraction_agent: String,
    /// Model for extraction workers. "haiku" for claude, "o3-mini" for codex.
    /// Empty string = fall back to `worker_model`.
    pub extraction_model: String,
    /// Enable auto-extraction workers on save
    pub auto_extract: bool,
    /// Enable nightly dreaming optimizer
    pub dreaming_enabled: bool,
    /// Agent provider for dreaming cron (default: same as general_agent)
    pub dreaming_agent: String,
    /// Model for dreaming agent. Empty string = fall back to `worker_model`.
    pub dreaming_model: String,
    /// Global default model for any mem worker (extraction + dreaming). Used
    /// when the more specific `extraction_model` / `dreaming_model` is empty.
    /// Defaults to claude-haiku-4-5-20251001 — the cheapest model that still
    /// reasons well enough for structured extraction.
    #[serde(default = "default_worker_model")]
    pub worker_model: String,
    /// Max concurrent workers per agent provider. Further spawns queue
    /// FIFO until a slot frees up.
    pub workers_per_provider: usize,
    /// Hard timeout for any hidden agent run (mem worker / cron). The auto-
    /// complete watcher marks the run Failed if claude hasn't emitted a
    /// stream-json result event within this many seconds. Default 150.
    #[serde(default = "default_worker_timeout_seconds")]
    pub worker_timeout_seconds: u64,
    #[serde(default = "default_dedup_threshold")]
    pub dedup_threshold: f64,
    pub file_sync_enabled: bool,
    #[serde(default = "default_file_sync_interval")]
    pub file_sync_interval_minutes: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            extraction_agent: String::new(),
            extraction_model: String::new(),
            auto_extract: true,
            dreaming_enabled: true,
            dreaming_agent: String::new(),
            dreaming_model: String::new(),
            worker_model: default_worker_model(),
            workers_per_provider: 3,
            worker_timeout_seconds: default_worker_timeout_seconds(),
            dedup_threshold: default_dedup_threshold(),
            file_sync_enabled: true,
            file_sync_interval_minutes: default_file_sync_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationsConfig {
    pub telegram_enabled: bool,
    pub telegram_allowed_users: Vec<String>,
    pub discord_enabled: bool,
    pub discord_allowed_users: Vec<String>,
    pub webhook_enabled: bool,
    pub webhook_port: u16,
    pub voice_transcribe: VoiceTranscribeConfig,
    pub secrets: SecretsConfig,
}

impl Default for IntegrationsConfig {
    fn default() -> Self {
        Self {
            telegram_enabled: false,
            telegram_allowed_users: Vec::new(),
            discord_enabled: false,
            discord_allowed_users: Vec::new(),
            webhook_enabled: false,
            webhook_port: 7687,
            voice_transcribe: VoiceTranscribeConfig::default(),
            secrets: SecretsConfig::default(),
        }
    }
}

fn default_secrets_backend() -> String {
    "local".to_string()
}

fn default_bitwarden_server_url() -> String {
    "https://vault.bitwarden.com".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SecretsConfig {
    /// Active backend id. Built-in:
    /// - `"local"` (default — secrets persisted in the OS keychain)
    /// - `"bitwarden"` (shells out to the Bitwarden CLI)
    /// - `"mock"` (in-memory, tests and dev only)
    #[serde(default = "default_secrets_backend")]
    pub backend: String,
    /// Default Bitwarden organization id for new items. Empty = personal vault.
    #[serde(default)]
    pub default_organization: String,
    /// Default Bitwarden collection id for new items.
    #[serde(default)]
    pub default_collection: String,
    /// Bitwarden adapter settings. Held in a sub-struct so future per-backend
    /// settings (e.g. `local.index_path`) can sit alongside without growing
    /// this top-level struct.
    #[serde(default)]
    pub bitwarden: BitwardenSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BitwardenSettings {
    /// Bitwarden / Vaultwarden server URL. Default
    /// `https://vault.bitwarden.com`. Self-hosted users override this. The
    /// adapter runs `bw config server <URL>` before login.
    #[serde(default = "default_bitwarden_server_url")]
    pub server_url: String,
}

impl Default for BitwardenSettings {
    fn default() -> Self {
        Self {
            server_url: default_bitwarden_server_url(),
        }
    }
}

impl Default for SecretsConfig {
    fn default() -> Self {
        Self {
            backend: default_secrets_backend(),
            default_organization: String::new(),
            default_collection: String::new(),
            bitwarden: BitwardenSettings::default(),
        }
    }
}

fn default_voice_daemon_url() -> String { "http://127.0.0.1:7891".to_string() }
fn default_voice_timeout_ms() -> u64 { 60_000 }
fn default_voice_health_ttl_secs() -> u64 { 60 }
fn default_voice_fallback_raw() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceTranscribeConfig {
    pub enabled: bool,
    #[serde(default = "default_voice_daemon_url")]
    pub daemon_url: String,
    #[serde(default = "default_voice_timeout_ms")]
    pub timeout_ms: u64,
    pub language: String,
    #[serde(default = "default_voice_fallback_raw")]
    pub fallback_raw_on_error: bool,
    #[serde(default = "default_voice_health_ttl_secs")]
    pub health_check_ttl_secs: u64,
}

impl Default for VoiceTranscribeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            daemon_url: default_voice_daemon_url(),
            timeout_ms: default_voice_timeout_ms(),
            language: String::new(),
            fallback_raw_on_error: default_voice_fallback_raw(),
            health_check_ttl_secs: default_voice_health_ttl_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MeshConfig {
    pub enabled: bool,
    pub instance_name: String,
    pub broadcast_duration_secs: u64,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            instance_name: hostname::get()
                .map(|h| {
                    let name = h.to_string_lossy().to_string();
                    // Strip .local suffix (macOS adds it)
                    name.trim_end_matches(".local").to_string()
                })
                .unwrap_or_else(|_| "CodeTerm".to_string()),
            broadcast_duration_secs: 180,
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
    Some(crate::paths::data_dir().join("themes"))
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
