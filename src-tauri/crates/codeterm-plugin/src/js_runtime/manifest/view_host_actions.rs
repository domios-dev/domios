use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::Capabilities;

/// A user-gesture action a plugin view may ask the host to perform (`capabilities.viewHostActions`).
/// The plugin only names it; the host checks the gesture, resolves any media handle and acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewHostAction {
    /// Put plain text on the clipboard.
    CopyText,
    /// Save one of the plugin's own media files where the user chooses.
    SaveAs,
    /// Show one of the plugin's own media files in the system file manager.
    Reveal,
}

impl Capabilities {
    /// Cross-field rules serde cannot express: host actions belong to a view, each named once.
    pub fn validate_view_host_actions(&self) -> Result<(), String> {
        if !self.view_host_actions.is_empty() && !self.view {
            return Err("viewHostActions requires the view capability".into());
        }
        let mut seen = HashSet::new();
        for action in &self.view_host_actions {
            if !seen.insert(action) {
                return Err(format!("viewHostActions names {action:?} twice"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::PluginManifest;
    use super::*;

    fn manifest(capabilities: serde_json::Value) -> Result<PluginManifest, String> {
        PluginManifest::from_json_str(
            &serde_json::json!({
                "id": "studio",
                "version": "0.1.0",
                "displayName": "Studio",
                "capabilities": capabilities,
            })
            .to_string(),
        )
    }

    #[test]
    fn view_host_actions_parse_every_declared_action() {
        let parsed = manifest(serde_json::json!({
            "view": true,
            "viewHostActions": ["copyText", "saveAs", "reveal"],
        }))
        .expect("valid host actions");
        assert_eq!(
            parsed.capabilities.view_host_actions,
            vec![
                ViewHostAction::CopyText,
                ViewHostAction::SaveAs,
                ViewHostAction::Reveal
            ]
        );
    }

    #[test]
    fn view_host_actions_default_to_none() {
        let parsed = manifest(serde_json::json!({ "view": true })).expect("plain view");
        assert!(parsed.capabilities.view_host_actions.is_empty());
    }

    #[test]
    fn view_host_actions_refuse_an_unknown_action() {
        let error = manifest(serde_json::json!({ "view": true, "viewHostActions": ["openUrl"] }))
            .expect_err("unknown action fails the load");
        assert!(error.contains("openUrl"), "{error}");
    }

    #[test]
    fn view_host_actions_require_a_view() {
        let error = manifest(serde_json::json!({ "viewHostActions": ["copyText"] }))
            .expect_err("no view to ask from");
        assert!(error.contains("requires the view capability"), "{error}");
    }

    #[test]
    fn view_host_actions_refuse_a_repeated_action() {
        let error = manifest(serde_json::json!({
            "view": true,
            "viewHostActions": ["saveAs", "saveAs"],
        }))
        .expect_err("duplicate action");
        assert!(error.contains("twice"), "{error}");
    }
}
