//! Declarative, host-rendered UI vocabulary shared by plugin surfaces (glance
//! popovers now; bubble detail later). Plugins return a `ViewNode` tree; the
//! host renders it with themed components. No raw HTML/JS/CSS, no iframes —
//! safe by construction. Styling is limited to a fixed token set (`NodeStyle`).
//!
//! Distinct from `agent::provider::SettingField` (form inputs); `ViewNode` is
//! read-only display.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Default,
    Muted,
    Ok,
    Warn,
    Danger,
    Accent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Spacing {
    None,
    Sm,
    #[default]
    Md,
    Lg,
}

/// The only styling a plugin may set — bounded tokens, never free CSS.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeStyle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<Tone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing: Option<Spacing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NoteLevel {
    #[default]
    Info,
    Warn,
    Error,
}

/// A node in a plugin-authored view tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ViewNode {
    /// A line of text.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<NodeStyle>,
    },
    /// A big value with a small caption (e.g. token count).
    Stat {
        label: String,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<NodeStyle>,
    },
    /// A label/value pair on one line (e.g. "Account: me@x.io").
    KeyVal {
        key: String,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<NodeStyle>,
    },
    /// A small pill.
    Badge {
        label: String,
        #[serde(default)]
        tone: Tone,
    },
    /// Horizontal layout.
    Row {
        children: Vec<ViewNode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<NodeStyle>,
    },
    /// Vertical layout.
    Stack {
        children: Vec<ViewNode>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<NodeStyle>,
    },
    /// A horizontal rule.
    Divider,
    /// An info/warn/error callout.
    Note {
        body: String,
        #[serde(default)]
        level: NoteLevel,
    },
    /// A button that emits an `action` id back to the host. Reserved ids:
    /// `settings` (open the plugin's settings) and `openPanel:<id>` (open a
    /// native view). Any other id is dispatched to the plugin's `glanceAction`.
    Button {
        label: String,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<NodeStyle>,
    },
}

/// A complete glance view: optional title + the node tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlanceView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub nodes: Vec<ViewNode>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_node_round_trips_with_kind_tag() {
        let v = GlanceView {
            title: Some("Bitwarden".into()),
            nodes: vec![
                ViewNode::KeyVal { key: "Account".into(), value: "me@x.io".into(), style: None },
                ViewNode::Badge { label: "Unlocked".into(), tone: Tone::Ok },
                ViewNode::Divider,
                ViewNode::Button { label: "Open settings".into(), action: "settings".into(), style: None },
            ],
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"kind\":\"keyVal\""));
        assert!(json.contains("\"kind\":\"badge\""));
        let back: GlanceView = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn style_tokens_serialize_snake_case() {
        let n = ViewNode::Text {
            text: "x".into(),
            style: Some(NodeStyle { tone: Some(Tone::Danger), spacing: Some(Spacing::Lg) }),
        };
        let json = serde_json::to_string(&n).unwrap();
        assert!(json.contains("\"tone\":\"danger\""));
        assert!(json.contains("\"spacing\":\"lg\""));
    }
}
