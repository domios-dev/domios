use std::path::{Path, PathBuf};

use crate::provider::SettingField;

use super::PluginManifest;

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
    out
}
