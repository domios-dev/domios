use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const DEFAULT_MAX_LINES: usize = 200;
const NAME_LIMIT: usize = 100;
const VALUE_LIMIT: usize = 60;

const INTERACTIVE: &[&str] = &[
    "button",
    "link",
    "textbox",
    "searchbox",
    "combobox",
    "checkbox",
    "radio",
    "switch",
    "slider",
    "spinbutton",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "option",
    "treeitem",
    "listbox",
];
const STRUCTURAL: &[&str] = &[
    "heading",
    "dialog",
    "alertdialog",
    "alert",
    "navigation",
    "main",
    "form",
    "list",
    "listitem",
    "img",
    "table",
    "row",
    "cell",
    "status",
];
const HIDDEN_ROLES: &[&str] = &[
    "none",
    "generic",
    "InlineTextBox",
    "StaticText",
    "RootWebArea",
    "ListMarker",
    "LineBreak",
];

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AxValue {
    #[serde(default)]
    pub value: Option<Value>,
}

impl AxValue {
    fn as_str(&self) -> &str {
        self.value.as_ref().and_then(Value::as_str).unwrap_or("")
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct AxProperty {
    pub name: String,
    #[serde(default)]
    pub value: AxValue,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxNode {
    pub node_id: String,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub role: Option<AxValue>,
    #[serde(default)]
    pub name: Option<AxValue>,
    #[serde(default)]
    pub value: Option<AxValue>,
    #[serde(default)]
    pub properties: Vec<AxProperty>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub child_ids: Vec<String>,
    #[serde(default, rename = "backendDOMNodeId")]
    pub backend_dom_node_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefEntry {
    #[serde(rename = "ref")]
    pub reference: String,
    pub role: String,
    pub name: String,
    pub backend_node_id: i64,
}

/// Refs map to backend DOM node ids and go stale on navigation; the owner drops the snapshot then.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub text: String,
    pub refs: HashMap<String, i64>,
    pub index: Vec<RefEntry>,
}

/// `result` is the `Accessibility.getFullAXTree` response body.
pub fn parse_full_ax_tree(result: &Value) -> Result<Vec<AxNode>, String> {
    let nodes = result
        .get("nodes")
        .cloned()
        .ok_or("Accessibility.getFullAXTree returned no nodes")?;
    serde_json::from_value(nodes).map_err(|error| format!("invalid AX tree: {error}"))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn display_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn build_snapshot(nodes: &[AxNode], max_lines: usize) -> Snapshot {
    let by_id: HashMap<&str, &AxNode> = nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();
    let root = nodes
        .iter()
        .find(|n| n.parent_id.as_deref().is_none_or(str::is_empty))
        .or_else(|| nodes.first());
    let mut snapshot = Snapshot::default();
    let mut lines: Vec<String> = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();
    let mut stack: Vec<(&AxNode, usize)> = root.map(|node| vec![(node, 0)]).unwrap_or_default();
    while let Some((node, depth)) = stack.pop() {
        if lines.len() >= max_lines {
            break;
        }
        if !visited.insert(node.node_id.as_str()) {
            continue;
        }
        let role = node.role.as_ref().map(AxValue::as_str).unwrap_or("");
        let name = collapse_whitespace(node.name.as_ref().map(AxValue::as_str).unwrap_or(""));
        let visible = !node.ignored && !HIDDEN_ROLES.contains(&role);
        let backend = node.backend_dom_node_id.filter(|id| *id != 0);
        let interactive = visible && INTERACTIVE.contains(&role) && backend.is_some();
        let mut next_depth = depth;
        if visible
            && (interactive
                || STRUCTURAL.contains(&role)
                || (!name.is_empty() && role != "paragraph"))
        {
            let mut line = format!("{}- {role}", "  ".repeat(depth));
            if !name.is_empty() {
                line.push_str(&format!(" \"{}\"", truncate_chars(&name, NAME_LIMIT)));
            }
            if let Some(value) = node.value.as_ref().and_then(|v| v.value.as_ref()) {
                let shown = display_value(value);
                if !shown.is_empty() {
                    line.push_str(&format!(": {}", truncate_chars(&shown, VALUE_LIMIT)));
                }
            }
            let disabled = node
                .properties
                .iter()
                .find(|property| property.name == "disabled")
                .and_then(|property| property.value.value.as_ref())
                .is_some_and(truthy);
            if disabled {
                line.push_str(" [disabled]");
            }
            if let (true, Some(backend)) = (interactive, backend) {
                let reference = format!("e{}", snapshot.index.len() + 1);
                snapshot.refs.insert(reference.clone(), backend);
                line.push_str(&format!(" [@{reference}]"));
                snapshot.index.push(RefEntry {
                    reference,
                    role: role.to_string(),
                    name: name.clone(),
                    backend_node_id: backend,
                });
            }
            lines.push(line);
            next_depth = depth + 1;
        }
        for child in node.child_ids.iter().rev() {
            if let Some(child) = by_id.get(child.as_str()) {
                stack.push((*child, next_depth));
            }
        }
    }
    if lines.len() >= max_lines {
        lines.push(format!(
            "… (truncated at {max_lines} lines; pass a larger limit)"
        ));
    }
    snapshot.text = lines.join("\n");
    snapshot
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub backend_node_id: i64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    NoSnapshot,
    StaleRef(String),
    MissingTarget,
    NotFound { role: String, name: String },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSnapshot => write!(f, "no snapshot yet — run `snapshot` first"),
            Self::StaleRef(reference) => {
                write!(f, "stale or unknown ref {reference} — run `snapshot` again")
            }
            Self::MissingTarget => write!(f, "expected @ref or <role> <name>"),
            Self::NotFound { role, name } => write!(
                f,
                "no {role} named \"{name}\" in the last snapshot — run `snapshot` and use a ref"
            ),
        }
    }
}

/// Resolve `@e3` or `(role, name)` against the latest snapshot: exact name first, then substring.
pub fn resolve_target(
    snapshot: Option<&Snapshot>,
    args: &[&str],
) -> Result<ResolvedTarget, ResolveError> {
    let snapshot = snapshot.ok_or(ResolveError::NoSnapshot)?;
    let first = args.first().copied().unwrap_or("");
    if let Some(reference) = first.strip_prefix('@') {
        return snapshot
            .refs
            .get(reference)
            .map(|id| ResolvedTarget {
                backend_node_id: *id,
                label: first.to_string(),
            })
            .ok_or_else(|| ResolveError::StaleRef(first.to_string()));
    }
    if first.is_empty() {
        return Err(ResolveError::MissingTarget);
    }
    let name = args.get(1).copied().unwrap_or("");
    let want = name.to_lowercase();
    let same_role = || snapshot.index.iter().filter(|entry| entry.role == first);
    same_role()
        .find(|entry| entry.name.to_lowercase() == want)
        .or_else(|| same_role().find(|entry| entry.name.to_lowercase().contains(&want)))
        .map(|entry| ResolvedTarget {
            backend_node_id: entry.backend_node_id,
            label: format!("{first} \"{}\"", entry.name),
        })
        .ok_or_else(|| ResolveError::NotFound {
            role: first.to_string(),
            name: name.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("fixtures/ax_tree_login.json");
    const EXPECTED: &str = "- navigation \"Primary\"
  - link \"Home\" [@e1]
  - link \"Pricing\" [@e2]
- main
  - heading \"Sign in to Acme\"
  - form
    - textbox \"Email\": andrey@example.com [@e3]
    - textbox \"Password\" [@e4]
    - checkbox \"Remember me\" [@e5]
    - slider \"Session length\": 30 [@e6]
    - button \"Sign in\" [@e7]
    - button \"Continue with SSO\" [disabled] [@e8]
    - link \"Forgot password?\" [@e9]
  - list
    - listitem
      - link \"Terms\"
    - listitem \"Status: all systems normal\"
      - img
      - tab \"Sign in\" [@e10]";

    fn fixture_nodes() -> Vec<AxNode> {
        let value: Value = serde_json::from_str(FIXTURE).unwrap();
        parse_full_ax_tree(&value).unwrap()
    }

    fn fixture_snapshot() -> Snapshot {
        build_snapshot(&fixture_nodes(), DEFAULT_MAX_LINES)
    }

    #[test]
    fn browser_snapshot_matches_poc_output_for_recorded_tree() {
        assert_eq!(fixture_snapshot().text, EXPECTED);
    }

    #[test]
    fn browser_snapshot_maps_refs_to_backend_node_ids() {
        let snapshot = fixture_snapshot();
        let expected = [
            ("e1", 9),
            ("e2", 11),
            ("e3", 24),
            ("e4", 25),
            ("e5", 26),
            ("e6", 27),
            ("e7", 28),
            ("e8", 29),
            ("e9", 31),
            ("e10", 45),
        ];
        assert_eq!(snapshot.refs.len(), expected.len());
        for (reference, id) in expected {
            assert_eq!(snapshot.refs.get(reference), Some(&id), "{reference}");
        }
        assert_eq!(snapshot.index[2].role, "textbox");
        assert_eq!(snapshot.index[2].name, "Email");
    }

    #[test]
    fn browser_snapshot_truncates_at_line_limit() {
        let snapshot = build_snapshot(&fixture_nodes(), 4);
        assert_eq!(
            snapshot.text,
            "- navigation \"Primary\"\n  - link \"Home\" [@e1]\n  - link \"Pricing\" [@e2]\n- main\n… (truncated at 4 lines; pass a larger limit)"
        );
        assert_eq!(snapshot.refs.len(), 2);
    }

    #[test]
    fn browser_snapshot_survives_cycles_and_empty_trees() {
        assert_eq!(build_snapshot(&[], DEFAULT_MAX_LINES).text, "");
        let cyclic: Vec<AxNode> = serde_json::from_value(serde_json::json!([
            {"nodeId": "a", "role": {"value": "button"}, "name": {"value": "Go"}, "childIds": ["b"], "backendDOMNodeId": 3},
            {"nodeId": "b", "parentId": "a", "role": {"value": "link"}, "name": {"value": "Back"}, "childIds": ["a"], "backendDOMNodeId": 4}
        ]))
        .unwrap();
        let snapshot = build_snapshot(&cyclic, DEFAULT_MAX_LINES);
        assert_eq!(
            snapshot.text,
            "- button \"Go\" [@e1]\n  - link \"Back\" [@e2]"
        );
    }

    #[test]
    fn browser_snapshot_truncates_long_names_by_chars() {
        let long = "→".repeat(150);
        let nodes: Vec<AxNode> = serde_json::from_value(serde_json::json!([
            {"nodeId": "a", "role": {"value": "heading"}, "name": {"value": long}}
        ]))
        .unwrap();
        let text = build_snapshot(&nodes, DEFAULT_MAX_LINES).text;
        assert_eq!(text, format!("- heading \"{}\"", "→".repeat(NAME_LIMIT)));
    }

    #[test]
    fn browser_resolve_by_ref_and_stale_ref() {
        let snapshot = fixture_snapshot();
        assert_eq!(
            resolve_target(Some(&snapshot), &["@e7"]),
            Ok(ResolvedTarget {
                backend_node_id: 28,
                label: "@e7".into()
            })
        );
        assert_eq!(
            resolve_target(Some(&snapshot), &["@e99"]),
            Err(ResolveError::StaleRef("@e99".into()))
        );
    }

    #[test]
    fn browser_resolve_by_role_and_name_prefers_exact_then_substring() {
        let snapshot = fixture_snapshot();
        let exact = resolve_target(Some(&snapshot), &["button", "sign in"]).unwrap();
        assert_eq!(exact.backend_node_id, 28);
        assert_eq!(exact.label, "button \"Sign in\"");
        let partial = resolve_target(Some(&snapshot), &["button", "sso"]).unwrap();
        assert_eq!(partial.backend_node_id, 29);
        let tab = resolve_target(Some(&snapshot), &["tab", "Sign in"]).unwrap();
        assert_eq!(tab.backend_node_id, 45);
    }

    #[test]
    fn browser_resolve_reports_typed_errors() {
        let snapshot = fixture_snapshot();
        assert_eq!(
            resolve_target(None, &["@e1"]),
            Err(ResolveError::NoSnapshot)
        );
        assert_eq!(
            resolve_target(Some(&snapshot), &[]),
            Err(ResolveError::MissingTarget)
        );
        assert_eq!(
            resolve_target(Some(&snapshot), &["button", "Delete account"]),
            Err(ResolveError::NotFound {
                role: "button".into(),
                name: "Delete account".into()
            })
        );
    }
}
