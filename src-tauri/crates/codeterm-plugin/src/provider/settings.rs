use serde::{Deserialize, Serialize};

use super::{ActionStyle, BadgeVariant, ModelInfo, NoteLevel, PathOpenKind};

/// Declarative settings UI primitives. The frontend ships ONE
/// `<SchemaRenderer />` component that knows how to render each variant.
/// Plugin authors describe their settings panel as a `Vec<SettingField>`
/// — no JS, no per-plugin React component, no hardcoded fallback.
///
/// **Schema completeness rule:** if a real plugin needs a UI pattern not
/// covered here, ADD A NEW VARIANT — never carve out an escape hatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SettingField {
    // ── Grouping ─────────────────────────────────────────────────
    /// Card with title + description + nested fields.
    Section {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        fields: Vec<SettingField>,
    },

    Toggle {
        /// Config path relative to the plugin's namespace
        /// (e.g. `"resume_sessions"` → `agents.<id>.resume_sessions`).
        key: String,
        label: String,
        default: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Text {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Select {
        key: String,
        label: String,
        /// `(value, display)` pairs.
        options: Vec<(String, String)>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Array {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<Vec<serde_json::Map<String, serde_json::Value>>>,
        item: ArraySettingItem,
    },
    /// Read-only field that reports an env var the user must set externally.
    ApiKey {
        env_var: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link: Option<String>,
    },

    // ── Display ──────────────────────────────────────────────────
    /// Static documentation block. Body supports inline `code` and links.
    Info {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        body: String,
        #[serde(default = "NoteLevel::default_info")]
        level: NoteLevel,
    },
    /// Filesystem path with an "Open folder/file" button.
    PathLink {
        label: String,
        path: String,
        open: PathOpenKind,
    },
    /// Polled status badge: frontend invokes `query_command` and renders
    /// the variant whose `id` matches the returned value. `query_args`
    /// is an optional name → value map forwarded as the invoke arguments
    /// (snake_case keys are remapped to camelCase by Tauri automatically).
    Badge {
        query_command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query_args: Option<serde_json::Value>,
        variants: Vec<BadgeVariant>,
    },

    // ── Actions ──────────────────────────────────────────────────
    /// Button that invokes a backend command. Optional `state_query`
    /// turns it into a multi-state button (Install / Reinstall / Update).
    /// Both `command` and `state_query` use the same `args` payload —
    /// pass `provider_id` here for generic dispatchers that target a
    /// specific provider.
    Action {
        id: String,
        label: String,
        command: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        state_query: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confirm: Option<String>,
        #[serde(default = "ActionStyle::default_primary")]
        style: ActionStyle,
    },

    // ── Conditional visibility ───────────────────────────────────
    /// Render `then` only when the named field equals `equals`.
    When {
        field: String,
        equals: serde_json::Value,
        then: Vec<SettingField>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArraySettingItem {
    Object { fields: Vec<ObjectSettingField> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjectSettingField {
    Text {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Textarea {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Number {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Json {
        key: String,
        label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<serde_json::Map<String, serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
}

impl NoteLevel {
    fn default_info() -> Self {
        NoteLevel::Info
    }
}

impl ActionStyle {
    fn default_primary() -> Self {
        ActionStyle::Primary
    }
}

// Reusable settings schema builders — shared across providers.

/// Build the "Session Resume" settings section common to all providers.
pub fn session_resume_section(description: &str, _skip_flag: &str) -> SettingField {
    SettingField::Section {
        title: "Session Resume".into(),
        description: Some(description.into()),
        fields: vec![
            SettingField::Toggle {
                key: "resume_sessions".into(),
                label: "Resume previous sessions".into(),
                default: true,
                description: None,
            },
        ],
    }
}

/// Build the "Usage Tracking" settings section. Auto-injected into the
/// schema for any provider that declares `usage_parser()` — providers
/// don't need to include it manually.
pub fn usage_tracking_section(probe_command: &str) -> SettingField {
    SettingField::Section {
        title: "Usage Tracking".into(),
        description: Some(format!(
            "Periodically probe {} to capture session + weekly limits and surface them in \
             orchestrator heartbeats and the usage pill. Enabled by default for usage-capable \
             providers; disable per provider if the probe interferes with active sessions.",
            probe_command
        )),
        fields: vec![SettingField::Toggle {
            key: "usage_tracking_enabled".into(),
            label: "Enable usage tracking".into(),
            default: true,
            description: None,
        }],
    }
}

/// Build the "Model" settings section from a provider's available models.
pub fn model_select_section(
    description: &str,
    models: Vec<ModelInfo>,
    default: Option<&str>,
) -> SettingField {
    SettingField::Section {
        title: "Model".into(),
        description: Some(description.into()),
        fields: vec![SettingField::Select {
            key: "default_model".into(),
            label: "Default model".into(),
            options: models.into_iter().map(|m| (m.id, m.display_name)).collect(),
            default: default.map(|s| s.to_string()),
            description: None,
        }],
    }
}
