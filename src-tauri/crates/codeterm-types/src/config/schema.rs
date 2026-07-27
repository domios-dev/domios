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
    /// Per-plugin settings, keyed by plugin id. A generic JSON bag because each
    /// plugin defines its own settings schema (rendered by SchemaRenderer); the
    /// plugin reads its own blob via `host.settingsJson()`.
    #[serde(default)]
    pub plugins: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub general_agent: GeneralAgentConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub orchestration: OrchestrationConfig,
    #[serde(default)]
    pub usage: UsageConfig,
    #[serde(default)]
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub mesh: MeshConfig,
    #[serde(default)]
    pub updater: UpdaterConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub runaway: RunawayConfig,
    #[serde(default)]
    pub kpi: KpiConfig,
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
            plugins: HashMap::new(),
            general_agent: GeneralAgentConfig::default(),
            memory: MemoryConfig::default(),
            orchestration: OrchestrationConfig::default(),
            usage: UsageConfig::default(),
            integrations: IntegrationsConfig::default(),
            mesh: MeshConfig::default(),
            updater: UpdaterConfig::default(),
            ui: UiConfig::default(),
            runaway: RunawayConfig::default(),
            kpi: KpiConfig::default(),
        }
    }
}

/// Orchestration-economics KPI event ledger. Off by default until phase 3 wires the
/// choke-point counters and phase 4 ships a query surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KpiConfig {
    pub enabled: bool,
}

impl Default for KpiConfig {
    fn default() -> Self {
        Self { enabled: false }
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
    /// Enable periodic /usage (or equivalent) probing for this provider.
    /// Default: None → false (disabled). Only meaningful for providers that
    /// declare a `usage_parser()`; ignored otherwise.
    pub usage_tracking_enabled: Option<bool>,
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
            port: std::env::var("CODETERM_API_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(7686),
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
    pub reopen_tab: String,
    pub close_tab: String,
    pub next_tab: String,
    pub prev_tab: String,
    pub close_tab: String,
    pub focus_tab_left: String,
    pub focus_tab_right: String,
    pub focus_tab_up: String,
    pub focus_tab_down: String,
    pub new_workspace: String,
    pub copy: String,
    pub paste: String,
    pub search: String,
    pub zoom_in: String,
    pub zoom_out: String,
    pub zoom_reset: String,
    pub open_settings: String,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            new_tab: "Ctrl+T".to_string(),
            reopen_tab: "Ctrl+Shift+T".to_string(),
            close_tab: "Ctrl+W".to_string(),
            next_tab: "Ctrl+Tab".to_string(),
            prev_tab: "Ctrl+Shift+Tab".to_string(),
            close_tab: "Ctrl+Shift+Q".to_string(),
            focus_tab_left: "Alt+ArrowLeft".to_string(),
            focus_tab_right: "Alt+ArrowRight".to_string(),
            focus_tab_up: "Alt+ArrowUp".to_string(),
            focus_tab_down: "Alt+ArrowDown".to_string(),
            new_workspace: "Ctrl+Shift+N".to_string(),
            copy: "Ctrl+Shift+C".to_string(),
            paste: "Ctrl+Shift+V".to_string(),
            search: "Ctrl+Shift+F".to_string(),
            zoom_in: "Ctrl+=".to_string(),
            zoom_out: "Ctrl+-".to_string(),
            zoom_reset: "Ctrl+0".to_string(),
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
    /// False until the user confirms a provider on the first-run picker.
    #[serde(default = "default_first_run_complete_for_deserialize")]
    pub first_run_complete: bool,
}

fn default_first_run_complete_for_deserialize() -> bool {
    true
}

impl Default for GeneralAgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_provider: "claude".to_string(),
            auto_restart: true,
            max_restart_attempts: 5,
            restart_delay_s: 5,
            first_run_complete: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_worker_model() -> String {
    "haiku".to_string()
}

fn default_dreaming_model() -> String {
    "sonnet".to_string()
}

fn default_worker_timeout_seconds() -> u64 {
    300
}

fn default_dreaming_timeout_seconds() -> u64 {
    3600
}

fn default_dedup_threshold() -> f64 {
    0.15
}

fn default_file_sync_interval() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestrationConfig {
    pub plan_reminders: bool,
    pub nudge_after_turns: u32,
    pub nudge_interval_secs: u64,
    pub idle_poke: IdlePokeConfig,
    pub reflect: ReflectConfig,
    pub digests: OrchestrationDigestsConfig,
    pub refresh: RefreshConfig,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            plan_reminders: true,
            nudge_after_turns: 12,
            nudge_interval_secs: 1800,
            idle_poke: IdlePokeConfig::default(),
            reflect: ReflectConfig::default(),
            digests: OrchestrationDigestsConfig::default(),
            refresh: RefreshConfig::default(),
        }
    }
}

fn default_refresh_handoff_timeout_secs() -> u64 {
    60
}

fn default_refresh_settle_timeout_secs() -> u64 {
    120
}

fn default_refresh_reorient_timeout_secs() -> u64 {
    30
}

fn default_refresh_verify_timeout_secs() -> u64 {
    180
}

/// The orchestrator refresh cycle (S1..S5). Section `[orchestration.refresh]`.
/// `after_reflection` stays OFF until the drill series passes — the manual
/// `codeterm orchestrator refresh` trigger is always available regardless.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RefreshConfig {
    /// Auto-run a refresh after a reflection block records its learning. OFF by default.
    pub after_reflection: bool,
    #[serde(default = "default_refresh_handoff_timeout_secs")]
    pub handoff_timeout_secs: u64,
    #[serde(default = "default_refresh_settle_timeout_secs")]
    pub settle_timeout_secs: u64,
    #[serde(default = "default_refresh_reorient_timeout_secs")]
    pub reorient_timeout_secs: u64,
    #[serde(default = "default_refresh_verify_timeout_secs")]
    pub verify_timeout_secs: u64,
}

impl Default for RefreshConfig {
    fn default() -> Self {
        Self {
            after_reflection: false,
            handoff_timeout_secs: default_refresh_handoff_timeout_secs(),
            settle_timeout_secs: default_refresh_settle_timeout_secs(),
            reorient_timeout_secs: default_refresh_reorient_timeout_secs(),
            verify_timeout_secs: default_refresh_verify_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DigestCondition {
    #[default]
    InReviewGt,
    InProgressPressure,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DigestRuleConfig {
    pub id: String,
    pub enabled: bool,
    pub condition: DigestCondition,
    pub threshold: u32,
    pub stalled_threshold: u32,
    pub rate_limit_secs: u64,
    pub template: String,
}

impl Default for DigestRuleConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            enabled: false,
            condition: DigestCondition::InReviewGt,
            threshold: 3,
            stalled_threshold: 1,
            rate_limit_secs: 3600,
            template: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestrationDigestsConfig {
    pub enabled: bool,
    pub rules: Vec<DigestRuleConfig>,
}

fn default_in_review_digest_template() -> String {
    "{count} reports await your verdict, oldest {oldest_h}h — judge with `codeterm plan task close {plan_id} <task_id>` or route to QA with `codeterm plan qa queue {plan_id} --task <task_id>`".into()
}

fn default_in_progress_digest_template() -> String {
    "{in_progress} lanes in flight ({stalled} stalled) — run `codeterm plan in-progress {plan_id}` then `codeterm plan reconcile {plan_id}`".into()
}

pub fn default_digest_rules() -> Vec<DigestRuleConfig> {
    vec![
        DigestRuleConfig {
            id: "in_review_pressure".into(),
            enabled: true,
            condition: DigestCondition::InReviewGt,
            threshold: 3,
            stalled_threshold: 0,
            rate_limit_secs: 3600,
            template: default_in_review_digest_template(),
        },
        DigestRuleConfig {
            id: "in_progress_pressure".into(),
            enabled: true,
            condition: DigestCondition::InProgressPressure,
            threshold: 8,
            stalled_threshold: 1,
            rate_limit_secs: 3600,
            template: default_in_progress_digest_template(),
        },
    ]
}

impl Default for OrchestrationDigestsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: default_digest_rules(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageConfig {
    pub enabled: bool,
    pub heartbeat: bool,
    pub freshness_secs: u64,
    pub standalone_cadence_secs: u64,
}

impl Default for UsageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            heartbeat: true,
            freshness_secs: 180,
            standalone_cadence_secs: 0,
        }
    }
}

fn default_reflect_every_gates() -> u32 {
    5
}

/// Cadence for the orchestrator reflection loop. Section `[orchestration.reflect]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReflectConfig {
    /// Emit a self-review block every N gate/bounce events. 0 disables the cadence trigger.
    #[serde(default = "default_reflect_every_gates")]
    pub every_gates: u32,
}

impl Default for ReflectConfig {
    fn default() -> Self {
        Self {
            every_gates: default_reflect_every_gates(),
        }
    }
}

fn default_idle_poke_enabled() -> bool {
    true
}

fn default_idle_poke_threshold_secs() -> u64 {
    150
}

fn default_idle_poke_live_lane_cap() -> u32 {
    3
}

fn default_idle_poke_max_per_day() -> u32 {
    4
}

fn default_idle_poke_backoff_base_secs() -> u64 {
    90
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IdlePokeConfig {
    #[serde(default = "default_idle_poke_enabled")]
    pub enabled: bool,
    #[serde(default = "default_idle_poke_threshold_secs")]
    pub idle_threshold_secs: u64,
    #[serde(default = "default_idle_poke_live_lane_cap")]
    pub live_lane_cap: u32,
    #[serde(default = "default_idle_poke_max_per_day")]
    pub max_per_day: u32,
    #[serde(default = "default_idle_poke_backoff_base_secs")]
    pub backoff_base_secs: u64,
}

impl Default for IdlePokeConfig {
    fn default() -> Self {
        Self {
            enabled: default_idle_poke_enabled(),
            idle_threshold_secs: default_idle_poke_threshold_secs(),
            live_lane_cap: default_idle_poke_live_lane_cap(),
            max_per_day: default_idle_poke_max_per_day(),
            backoff_base_secs: default_idle_poke_backoff_base_secs(),
        }
    }
}

fn default_l1_budget_tokens() -> usize {
    300
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
    /// Agent provider for dreaming cron (default: same as general_agent)
    pub dreaming_agent: String,
    /// Model for dreaming agent. Defaults to "sonnet" — dreaming needs careful
    /// reasoning over hub topology, haiku tends to give up. Empty string falls
    /// back to `worker_model` (haiku).
    #[serde(default = "default_dreaming_model")]
    pub dreaming_model: String,
    /// Global default model for any mem worker (extraction + dreaming). Used
    /// when the more specific `extraction_model` / `dreaming_model` is empty.
    /// Defaults to "haiku" (resolves to current concrete id at runtime via
    /// agent::models::resolve_model_id).
    #[serde(default = "default_worker_model")]
    pub worker_model: String,
    /// Max concurrent workers per agent provider. Further spawns queue
    /// FIFO until a slot frees up.
    pub workers_per_provider: usize,
    /// Hard timeout for non-dreaming hidden agent runs (extraction workers,
    /// generic cron). The auto-complete watcher marks the run Failed if the
    /// agent hasn't emitted a stream-json result event within this many seconds.
    /// Defaults to 300 (5 min). Dreaming uses `dreaming_timeout_seconds`
    /// instead — dreaming needs longer to reason over hub decompositions.
    #[serde(default = "default_worker_timeout_seconds")]
    pub worker_timeout_seconds: u64,
    /// Hard timeout for the Dreamer (REM phase) specifically. Default 1800
    /// (30 min) because hub decomposition + diary + classification all in one
    /// session needs room to think. Set lower if your dreaming model is fast.
    #[serde(default = "default_dreaming_timeout_seconds")]
    pub dreaming_timeout_seconds: u64,
    #[serde(default = "default_dedup_threshold")]
    pub dedup_threshold: f64,
    pub file_sync_enabled: bool,
    #[serde(default = "default_file_sync_interval")]
    pub file_sync_interval_minutes: u64,
    #[serde(default = "default_true")]
    pub librarian_enabled: bool,
    #[serde(default = "default_l1_budget_tokens")]
    pub l1_budget_tokens: usize,
    /// Whether the nightly dreamer pushes its one-line "🧠 Dreamer brief"
    /// insight to the Telegram bridge after writing the diary. Default
    /// `true` (preserves existing behaviour). Flip to `false` if you find
    /// the nightly Telegram ping noisy — the diary in mem stays the
    /// durable record either way.
    #[serde(default = "default_true")]
    pub dreaming_telegram_notify: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            extraction_agent: String::new(),
            extraction_model: String::new(),
            auto_extract: true,
            dreaming_agent: String::new(),
            dreaming_model: default_dreaming_model(),
            worker_model: default_worker_model(),
            workers_per_provider: 3,
            worker_timeout_seconds: default_worker_timeout_seconds(),
            dreaming_timeout_seconds: default_dreaming_timeout_seconds(),
            dedup_threshold: default_dedup_threshold(),
            file_sync_enabled: true,
            file_sync_interval_minutes: default_file_sync_interval(),
            librarian_enabled: true,
            l1_budget_tokens: default_l1_budget_tokens(),
            dreaming_telegram_notify: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub memory_tab_enabled: bool,
    #[serde(default = "default_true")]
    pub mesh_tab_enabled: bool,
    /// Centralized per-user client settings that used to live in each client's
    /// localStorage (prompt-seen flags, toggles). Pluggable; no Rust field churn.
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            memory_tab_enabled: true,
            mesh_tab_enabled: true,
            extra: HashMap::new(),
        }
    }
}

/// Accept user-id lists as either quoted strings (`["123"]`) or bare
/// integers (`[123]`) in TOML. Telegram/Discord chat ids are big ints,
/// so unquoted integers are the obvious thing for users to type. The
/// runtime keys off `i64.to_string()`, so we coerce to `Vec<String>`.
fn deserialize_user_id_list<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt {
        Str(String),
        Int(i64),
    }
    let list: Vec<StringOrInt> = Vec::deserialize(d)?;
    Ok(list
        .into_iter()
        .map(|v| match v {
            StringOrInt::Str(s) => s,
            StringOrInt::Int(i) => i.to_string(),
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrationsConfig {
    pub telegram_enabled: bool,
    #[serde(deserialize_with = "deserialize_user_id_list")]
    pub telegram_allowed_users: Vec<String>,
    pub discord_enabled: bool,
    #[serde(deserialize_with = "deserialize_user_id_list")]
    pub discord_allowed_users: Vec<String>,
    pub webhook_enabled: bool,
    pub webhook_port: u16,
    pub voice_transcribe: VoiceTranscribeConfig,
    pub text_hooks: TextHooksConfig,
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
            text_hooks: TextHooksConfig::default(),
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
    /// Active backend id. The `SecretBackendFactory` registry knows the
    /// full list. Built-in: `"local"` (default), `"bitwarden"`, `"mock"`.
    #[serde(default = "default_secrets_backend")]
    pub backend: String,
    /// Default org id for new items (Bitwarden). Empty = personal vault.
    #[serde(default)]
    pub default_organization: String,
    /// Default collection id for new items (Bitwarden).
    #[serde(default)]
    pub default_collection: String,
    /// Bitwarden-specific settings. New per-backend structs go alongside
    /// (e.g. `vault: VaultSettings`) — never grow this struct in-place.
    #[serde(default)]
    pub bitwarden: BitwardenSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BitwardenSettings {
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

fn default_voice_daemon_url() -> String {
    "http://127.0.0.1:7891".to_string()
}
fn default_voice_timeout_ms() -> u64 {
    60_000
}
fn default_voice_health_ttl_secs() -> u64 {
    60
}
fn default_voice_fallback_raw() -> bool {
    true
}

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
    /// Id of the active transcriber in the registry. Defaults to the bundled
    /// `transcriber` plugin (no native engine); another plugin sets its own id.
    #[serde(default = "default_transcribe_active")]
    pub active: String,
}

pub fn default_transcribe_active() -> String {
    "transcriber".to_string()
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
            active: default_transcribe_active(),
        }
    }
}

fn default_text_hooks_url() -> String {
    "http://127.0.0.1:7892".to_string()
}
fn default_text_hooks_timeout_ms() -> u64 {
    2000
}
fn default_text_hooks_fallback() -> String {
    "forward".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TextHooksConfig {
    pub enabled: bool,
    #[serde(default = "default_text_hooks_url")]
    pub daemon_url: String,
    #[serde(default = "default_text_hooks_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_voice_health_ttl_secs")]
    pub health_check_ttl_secs: u64,
    #[serde(default = "default_text_hooks_fallback")]
    pub fallback_on_error: String,
}

impl Default for TextHooksConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            daemon_url: default_text_hooks_url(),
            timeout_ms: default_text_hooks_timeout_ms(),
            health_check_ttl_secs: default_voice_health_ttl_secs(),
            fallback_on_error: default_text_hooks_fallback(),
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
            enabled: true,
            instance_name: String::new(),
            broadcast_duration_secs: 180,
        }
    }
}

fn default_runaway_enabled() -> bool {
    true
}

fn default_test_binary_max_secs() -> u64 {
    900
}

/// Config for the daemon runaway-test-binary sweeper.
/// Section `[runaway]` in config.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RunawayConfig {
    /// Enable the background sweeper. Default true.
    #[serde(default = "default_runaway_enabled")]
    pub enabled: bool,
    /// Kill test binaries (*/target/*/deps/*) that have been alive longer than this.
    #[serde(default = "default_test_binary_max_secs")]
    pub test_binary_max_secs: u64,
}

impl Default for RunawayConfig {
    fn default() -> Self {
        Self {
            enabled: default_runaway_enabled(),
            test_binary_max_secs: default_test_binary_max_secs(),
        }
    }
}

#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
