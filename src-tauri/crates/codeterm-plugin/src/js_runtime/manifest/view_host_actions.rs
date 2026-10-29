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
    /// Let the user choose files of the declared `viewInputs` kinds; the view gets opaque handles.
    PickFile,
}

pub const MAX_VIEW_INPUT_FILES: u8 = 8;

/// What `pickFile` may bring into the plugin (`capabilities.viewInputs`): passive media of the
/// listed kinds, each at most `maxBytes`, at most `maxFiles` per pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewInputs {
    pub accept: Vec<crate::media_delivery::MediaType>,
    pub max_bytes: u64,
    pub max_files: u8,
}

impl Capabilities {
    /// Cross-field rules serde cannot express: host actions belong to a view, each named once, and
    /// `pickFile` comes with bounded `viewInputs` (and only then).
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
        let picks = self.view_host_actions.contains(&ViewHostAction::PickFile);
        match (&self.view_inputs, picks) {
            (None, true) => Err("viewHostActions pickFile requires viewInputs".into()),
            (Some(_), false) => Err("viewInputs requires the pickFile view host action".into()),
            (Some(inputs), true) => inputs.validate(),
            (None, false) => Ok(()),
        }
    }
}

impl ViewInputs {
    fn validate(&self) -> Result<(), String> {
        let unique = self
            .accept
            .iter()
            .enumerate()
            .all(|(i, kind)| !self.accept[..i].contains(kind));
        if self.accept.is_empty() || !unique {
            return Err("viewInputs.accept must list each media type once".into());
        }
        if self.max_bytes == 0 || self.max_bytes > crate::media_delivery::MAX_MEDIA_BYTES {
            return Err(format!(
                "viewInputs.maxBytes must be 1-{}",
                crate::media_delivery::MAX_MEDIA_BYTES
            ));
        }
        if self.max_files == 0 || self.max_files > MAX_VIEW_INPUT_FILES {
            return Err(format!("viewInputs.maxFiles must be 1-{MAX_VIEW_INPUT_FILES}"));
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

    fn picking(inputs: serde_json::Value) -> Result<PluginManifest, String> {
        manifest(serde_json::json!({ "view": true, "viewHostActions": ["pickFile"], "viewInputs": inputs }))
    }

    #[test]
    fn pick_file_parses_bounded_passive_inputs() {
        let parsed = picking(serde_json::json!({
            "accept": ["image/png", "image/jpeg"], "maxBytes": 20_971_520, "maxFiles": 4,
        }))
        .expect("bounded inputs");
        let inputs = parsed.capabilities.view_inputs.expect("declared inputs");
        assert_eq!(
            inputs.accept,
            vec![
                crate::media_delivery::MediaType::Png,
                crate::media_delivery::MediaType::Jpeg
            ]
        );
        assert_eq!((inputs.max_bytes, inputs.max_files), (20_971_520, 4));
    }

    #[test]
    fn pick_file_and_view_inputs_require_each_other() {
        let error = manifest(serde_json::json!({ "view": true, "viewHostActions": ["pickFile"] }))
            .expect_err("pickFile without inputs");
        assert!(error.contains("requires viewInputs"), "{error}");
        let error = manifest(serde_json::json!({
            "view": true,
            "viewInputs": { "accept": ["image/png"], "maxBytes": 1, "maxFiles": 1 },
        }))
        .expect_err("inputs without pickFile");
        assert!(error.contains("requires the pickFile"), "{error}");
    }

    #[test]
    fn view_inputs_refuse_active_unbounded_or_repeated_kinds() {
        for (inputs, needle) in [
            (serde_json::json!({ "accept": ["image/svg+xml"], "maxBytes": 1, "maxFiles": 1 }), "image/svg+xml"),
            (serde_json::json!({ "accept": ["text/html"], "maxBytes": 1, "maxFiles": 1 }), "text/html"),
            (serde_json::json!({ "accept": [], "maxBytes": 1, "maxFiles": 1 }), "each media type once"),
            (serde_json::json!({ "accept": ["image/png", "image/png"], "maxBytes": 1, "maxFiles": 1 }), "each media type once"),
            (serde_json::json!({ "accept": ["image/png"], "maxBytes": 0, "maxFiles": 1 }), "maxBytes"),
            (
                serde_json::json!({ "accept": ["image/png"], "maxBytes": crate::media_delivery::MAX_MEDIA_BYTES + 1, "maxFiles": 1 }),
                "maxBytes",
            ),
            (serde_json::json!({ "accept": ["image/png"], "maxBytes": 1, "maxFiles": 9 }), "maxFiles"),
            (serde_json::json!({ "accept": ["image/png"], "maxBytes": 1, "maxFiles": 1, "path": "/tmp" }), "path"),
        ] {
            let error = picking(inputs).expect_err("bounded passive inputs only");
            assert!(error.contains(needle), "{needle}: {error}");
        }
    }
}
