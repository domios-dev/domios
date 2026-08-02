use std::path::{Path, PathBuf};

use crate::provider::{ActionStyle, PathOpenKind, SettingField};

use super::{IntegrationKind, PluginManifest};

impl PluginManifest {
    /// Parse a manifest from a `plugin.json` file on disk.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut m: PluginManifest =
            serde_json::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        m.source_path = Some(path.to_path_buf());
        Ok(m)
    }

    /// Directory the manifest lives in — `plugin.js` resolves relative
    /// to this.
    pub fn dir(&self) -> Option<PathBuf> {
        self.source_path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    }

    /// Absolute path to the bundle's `plugin.js`.
    pub fn entry_path(&self) -> Option<PathBuf> {
        self.dir().map(|d| d.join(&self.entry))
    }

    /// Build the settings schema fields contributed by manifest data.
    /// Plugin's settingsSchema() may add more.
    pub fn applies_to_platform(&self, platform: &str) -> bool {
        let _ = platform;
        true // platform-targeted plugins are out of scope for v1
    }

    /// Expand a `~/...` path to the user's home directory.
    pub fn expand_home(path: &str) -> Option<PathBuf> {
        if let Some(rest) = path.strip_prefix("~/") {
            dirs::home_dir().map(|h| h.join(rest))
        } else if path == "~" {
            dirs::home_dir()
        } else {
            Some(PathBuf::from(path))
        }
    }
}

/// Build the standard `SettingField` sections every provider with a
/// session-resume + model story gets for free. Plugin's `settingsSchema()`
/// adds provider-specific sections on top.
pub fn manifest_settings_sections(m: &PluginManifest) -> Vec<SettingField> {
    let mut out = Vec::new();
    if let Some(flag) = m.commands.auto_approve_flags.first() {
        out.push(crate::provider::session_resume_section(
            &format!(
                "Automatically resume {} sessions when CodeTerm restarts.",
                m.display_name
            ),
            flag,
        ));
    }
    let models = crate::model_catalog::resolve_models(&m.id);
    if !models.is_empty() {
        out.push(crate::provider::model_select_section(
            &format!("Default model for new {} sessions.", m.display_name),
            models,
            crate::model_catalog::resolve_default(&m.id).as_deref(),
        ));
    }
    if let Some(integration) = m
        .integration
        .as_ref()
        .filter(|integration| !matches!(integration.kind, IntegrationKind::None))
    {
        let provider_id = m.id.clone();
        let target = integration.settings_path.clone();
        out.push(SettingField::Section {
            title: "Provider integration".into(),
            description: Some(format!(
                "Review and explicitly consent before CodeTerm changes {target}. Install creates a recoverable backup and preserves foreign configuration."
            )),
            fields: vec![
                SettingField::PathLink {
                    label: "Target configuration".into(),
                    path: target.clone(),
                    open: PathOpenKind::OpenFolder,
                },
                SettingField::Action {
                    id: "install_integration".into(),
                    label: "Review and install".into(),
                    command: "install_provider_integration".into(),
                    state_query: Some("check_provider_integration".into()),
                    args: Some(serde_json::json!({
                        "provider_id": provider_id,
                        "consent": true,
                        "precompact_memory_export": false
                    })),
                    confirm: Some(format!(
                        "Back up and merge CodeTerm callbacks into {target}? Foreign configuration is preserved."
                    )),
                    style: ActionStyle::Primary,
                },
                SettingField::Action {
                    id: "uninstall_integration".into(),
                    label: "Uninstall CodeTerm entries".into(),
                    command: "uninstall_provider_integration".into(),
                    state_query: None,
                    args: Some(serde_json::json!({ "provider_id": m.id })),
                    confirm: Some("Remove only CodeTerm-owned integration entries?".into()),
                    style: ActionStyle::Secondary,
                },
                SettingField::Action {
                    id: "restore_integration".into(),
                    label: "Restore pre-install backup".into(),
                    command: "restore_provider_integration".into(),
                    state_query: None,
                    args: Some(serde_json::json!({ "provider_id": m.id })),
                    confirm: Some(format!("Restore the backup over the current {target}?")),
                    style: ActionStyle::Danger,
                },
            ],
        });
    }
    out
}
