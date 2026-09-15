use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::Capabilities;

pub const MAX_TAB_MENU_ITEMS: usize = 4;
const MAX_ITEM_ID_BYTES: usize = 32;
const MAX_LABEL_CHARS: usize = 48;

/// A host-owned action. The plugin names it; only the host decides what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabMenuAction {
    /// Toggle the clicked tab as the target of the plugin's own webhook receiver.
    ReceiverTarget,
}

impl TabMenuAction {
    fn required_capability(self, caps: &Capabilities) -> Option<&'static str> {
        match self {
            Self::ReceiverTarget => caps.webhook_receiver.is_none().then_some("webhookReceiver"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TabMenuItem {
    pub id: String,
    pub label: String,
    pub action: TabMenuAction,
}

fn valid_item_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ITEM_ID_BYTES
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_label(label: &str) -> bool {
    let trimmed = label.trim();
    !trimmed.is_empty()
        && trimmed == label
        && label.chars().count() <= MAX_LABEL_CHARS
        && !label.chars().any(char::is_control)
}

impl Capabilities {
    /// Cross-field rules serde cannot express: bounded, unique items whose action the plugin is entitled to.
    pub fn validate_tab_menu(&self) -> Result<(), String> {
        if self.tab_menu.len() > MAX_TAB_MENU_ITEMS {
            return Err(format!(
                "tabMenu declares {} items; at most {MAX_TAB_MENU_ITEMS} are allowed",
                self.tab_menu.len()
            ));
        }
        let mut seen = HashSet::new();
        for item in &self.tab_menu {
            if !valid_item_id(&item.id) {
                return Err(format!(
                    "tabMenu item id '{}' must be 1-{MAX_ITEM_ID_BYTES} lowercase letters, digits or '-'",
                    item.id
                ));
            }
            if !seen.insert(item.id.as_str()) {
                return Err(format!("tabMenu item id '{}' is declared twice", item.id));
            }
            if !valid_label(&item.label) {
                return Err(format!(
                    "tabMenu item '{}' needs a trimmed label of 1-{MAX_LABEL_CHARS} characters without control characters",
                    item.id
                ));
            }
            if let Some(capability) = item.action.required_capability(self) {
                return Err(format!(
                    "tabMenu item '{}' uses an action that requires the {capability} capability",
                    item.id
                ));
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
                "id": "receiver",
                "version": "0.1.0",
                "displayName": "Receiver",
                "capabilities": capabilities,
            })
            .to_string(),
        )
    }

    fn receiver_menu(item: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "webhookReceiver": { "kind": "pebble" }, "tabMenu": [item] })
    }

    fn item() -> serde_json::Value {
        serde_json::json!({ "id": "receiver-target", "label": "Receive events here", "action": "receiverTarget" })
    }

    #[test]
    fn pebble_receiver_target_manifest_accepts_declared_menu_item() {
        let parsed = manifest(receiver_menu(item())).expect("valid tab menu");
        assert_eq!(
            parsed.capabilities.tab_menu,
            vec![TabMenuItem {
                id: "receiver-target".into(),
                label: "Receive events here".into(),
                action: TabMenuAction::ReceiverTarget,
            }]
        );
        let round_trip = serde_json::to_value(&parsed.capabilities).expect("serialize");
        assert_eq!(round_trip["tabMenu"][0], item());
    }

    #[test]
    fn pebble_receiver_target_manifest_without_menu_serializes_unchanged() {
        let parsed = manifest(serde_json::json!({ "view": true })).expect("no tab menu");
        let value = serde_json::to_value(&parsed.capabilities).expect("serialize");
        assert!(value.get("tabMenu").is_none());
    }

    #[test]
    fn pebble_receiver_target_manifest_rejects_action_outside_declared_capability() {
        let error = manifest(serde_json::json!({ "tabMenu": [item()] }))
            .expect_err("receiverTarget without webhookReceiver");
        assert!(error.contains("webhookReceiver"), "got: {error}");
    }

    #[test]
    fn pebble_receiver_target_manifest_rejects_malformed_items() {
        let malformed = [
            serde_json::json!({ "id": "receiver-target", "label": "Here", "action": "closeTab" }),
            serde_json::json!({ "id": "receiver-target", "label": "Here", "action": "receiverTarget", "tabId": "t" }),
            serde_json::json!({ "id": "receiver-target", "action": "receiverTarget" }),
            serde_json::json!({ "id": "Receiver Target", "label": "Here", "action": "receiverTarget" }),
            serde_json::json!({ "id": "", "label": "Here", "action": "receiverTarget" }),
            serde_json::json!({ "id": "receiver-target", "label": "  ", "action": "receiverTarget" }),
            serde_json::json!({ "id": "receiver-target", "label": "Here\u{7}", "action": "receiverTarget" }),
            serde_json::json!({ "id": "receiver-target", "label": "x".repeat(MAX_LABEL_CHARS + 1), "action": "receiverTarget" }),
        ];
        for item in malformed {
            assert!(
                manifest(receiver_menu(item.clone())).is_err(),
                "accepted malformed item {item}"
            );
        }
    }

    #[test]
    fn pebble_receiver_target_manifest_rejects_duplicate_and_excess_items() {
        let duplicate = serde_json::json!({
            "webhookReceiver": { "kind": "pebble" },
            "tabMenu": [item(), item()],
        });
        assert!(manifest(duplicate).is_err());

        let excess: Vec<serde_json::Value> = (0..=MAX_TAB_MENU_ITEMS)
            .map(|index| {
                serde_json::json!({ "id": format!("item-{index}"), "label": "Here", "action": "receiverTarget" })
            })
            .collect();
        let excess =
            serde_json::json!({ "webhookReceiver": { "kind": "pebble" }, "tabMenu": excess });
        assert!(manifest(excess).is_err());
    }
}
