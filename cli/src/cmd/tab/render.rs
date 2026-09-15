/// Reads a string field preserving case — for display, not fuzzy matching.
fn sfield<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

/// One-line human row used by both `Tab list` (when we switch off the
/// default kv printer) and `Tab find`. Shows id  tab/title  [provider:model]
/// so the user gets the same info they see in the sidebar.
pub fn format_tab_row(p: &serde_json::Value) -> String {
    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    // One label — the sidebar name. Tab title wins (that's what the user sees and what `tab rename` updates); falls back to tab.title only when the tab title is missing.
    let label = {
        let tab_title = p.get("tab_title").and_then(|v| v.as_str()).unwrap_or("");
        let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if !tab_title.is_empty() {
            tab_title
        } else {
            title
        }
    };
    let agent_label = super::labels::agent_label(p).unwrap_or_else(|| "-".to_string());
    let description = p.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let description_part = format_description_suffix(description);
    let ws = p
        .get("workspace_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let ws_part = if ws.is_empty() {
        String::new()
    } else {
        format!("  ws={}", ws)
    };
    format!(
        "{}  {}{} [{}]{}",
        id, label, description_part, agent_label, ws_part
    )
}

/// Human `Tab list`: workspaces as headers, tabs as `<id> <name> <model>`
/// rows, sub-agents nested under their parent by `parent_tab_id`. Compact
/// on purpose — this output is read by agents too, so every line costs tokens.
pub fn render_tab_tree(tabs: &[serde_json::Value]) -> String {
    render_tab_tree_with_current(tabs, None)
}

pub fn render_tab_tree_with_scope(
    tabs: &[serde_json::Value],
    current_tab_id: Option<&str>,
    requested_workspace: Option<&str>,
) -> String {
    let scope = requested_workspace.map(|_| "workspace").or_else(|| {
        tabs.first()
            .and_then(|tab| tab.get("scope"))
            .and_then(serde_json::Value::as_str)
    });
    let Some("workspace") = scope else {
        return if tabs.is_empty() {
            "(no tabs)\n".to_string()
        } else {
            render_tab_tree_with_current(tabs, current_tab_id)
        };
    };
    let workspace_id = tabs
        .first()
        .and_then(|tab| tab.get("scope_workspace_id"))
        .and_then(serde_json::Value::as_str)
        .or(requested_workspace)
        .unwrap_or("?");
    let workspace_name = tabs
        .first()
        .and_then(|tab| tab.get("scope_workspace_name"))
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .or_else(|| {
            tabs.first()
                .and_then(|tab| tab.get("workspace_name"))
                .and_then(serde_json::Value::as_str)
                .filter(|name| !name.is_empty())
        })
        .unwrap_or(workspace_id);
    let other_workspace_tab_count = tabs
        .first()
        .and_then(|tab| tab.get("other_workspace_tab_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let tree = if tabs.is_empty() {
        "(no tabs)\n".to_string()
    } else {
        render_tab_tree_with_current(tabs, current_tab_id)
    };
    format!(
        "scope: workspace {} ({}) — {} tabs in other workspaces\n\n{}",
        workspace_name, workspace_id, other_workspace_tab_count, tree,
    )
}

pub fn render_tab_tree_with_current(
    tabs: &[serde_json::Value],
    current_tab_id: Option<&str>,
) -> String {
    struct Row {
        id: String,
        label: String,
        model: String,
        description: String,
        parent: Option<String>,
        role: String,
        peer: String,
        team_id: String,
        creator_tab_id: String,
        is_current: bool,
    }
    // HashMap-unordered).
    let mut groups: std::collections::BTreeMap<(String, String), Vec<Row>> =
        std::collections::BTreeMap::new();
    for p in tabs {
        let id = sfield(p, "id").to_string();
        if id.is_empty() {
            continue;
        }
        let tab_title = sfield(p, "tab_title");
        let label = if !tab_title.is_empty() {
            tab_title
        } else {
            sfield(p, "title")
        }
        .to_string();
        let model = super::labels::agent_label(p).unwrap_or_default();
        let description = sfield(p, "description").to_string();
        let role = super::labels::role_label(p);
        let peer = sfield(p, "remote_peer").to_string();
        let team_id = sfield(p, "team_id").to_string();
        let creator_tab_id = sfield(p, "spawned_by_tab_id").to_string();
        let parent = p
            .get("parent_tab_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ws_name = sfield(p, "workspace_name").to_string();
        let ws_id = sfield(p, "workspace_id").to_string();
        groups.entry((ws_name, ws_id)).or_default().push(Row {
            is_current: current_tab_id == Some(id.as_str()),
            id,
            label,
            model,
            description,
            parent,
            role,
            peer,
            team_id,
            creator_tab_id,
        });
    }

    let mut out = String::new();
    for (gi, ((ws_name, ws_id), rows)) in groups.iter().enumerate() {
        if gi > 0 {
            out.push('\n');
        }
        let name = if ws_name.is_empty() {
            "(detached)"
        } else {
            ws_name.as_str()
        };
        if ws_id.is_empty() {
            out.push_str(&format!("{}\n", name));
        } else {
            out.push_str(&format!("{}  {}\n", name, ws_id));
        }

        // Nest by parent within the workspace. A tab whose parent lives in another workspace (or has none) is a root here.
        let ids: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let mut children: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        let mut roots: Vec<usize> = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            match &r.parent {
                Some(par) if ids.contains(par.as_str()) => {
                    children.entry(par.as_str()).or_default().push(i);
                }
                _ => roots.push(i),
            }
        }
        let label_w = rows
            .iter()
            .map(|r| r.label.chars().count())
            .max()
            .unwrap_or(0)
            .min(24);
        let labels_by_id: std::collections::HashMap<&str, &str> = rows
            .iter()
            .map(|row| (row.id.as_str(), row.label.as_str()))
            .collect();
        let row_rank = |row: &Row| {
            if !row.team_id.is_empty() && row.role.is_empty() {
                2
            } else if row.role == "worker" {
                1
            } else {
                0
            }
        };
        let by_label = |a: &usize, b: &usize| {
            row_rank(&rows[*a])
                .cmp(&row_rank(&rows[*b]))
                .then_with(|| rows[*a].label.cmp(&rows[*b].label))
        };
        roots.sort_by(by_label);

        // Iterative DFS so deep nesting can't blow the stack.
        let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&i| (i, 0usize)).collect();
        while let Some((i, depth)) = stack.pop() {
            let r = &rows[i];
            let indent = "  ".repeat(depth + 1);
            let current_suffix = if r.is_current { " (self)" } else { "" };
            let utility_suffix = if !r.team_id.is_empty() && r.role.is_empty() {
                let creator = labels_by_id
                    .get(r.creator_tab_id.as_str())
                    .copied()
                    .filter(|label| !label.is_empty())
                    .map(|label| format!("terminal by {label}"))
                    .unwrap_or_else(|| {
                        let id = if r.creator_tab_id.is_empty() {
                            "unknown"
                        } else {
                            r.creator_tab_id.as_str()
                        };
                        format!("terminal by archived {id}")
                    });
                Some(creator)
            } else {
                None
            };
            // Role and peer are their own axes; the bracket is the agent
            // identity and carries neither.
            let role_suffix = match (
                utility_suffix.as_deref().unwrap_or(r.role.as_str()),
                r.peer.as_str(),
            ) {
                ("", "") => String::new(),
                (role, "") => format!(" \u{b7}{}", role),
                ("", peer) => format!(" @{}", peer),
                (role, peer) => format!(" \u{b7}{} @{}", role, peer),
            };
            let bracket = if r.model.is_empty() {
                String::new()
            } else {
                format!("[{}]", r.model)
            };
            if bracket.is_empty() {
                out.push_str(&format!(
                    "{}{} {}{}{}{}\n",
                    indent,
                    r.id,
                    r.label,
                    format_description_suffix(&r.description),
                    role_suffix,
                    current_suffix
                ));
            } else {
                out.push_str(&format!(
                    "{}{} {:<w$} {}{}{}{}\n",
                    indent,
                    r.id,
                    r.label,
                    bracket,
                    format_description_suffix(&r.description),
                    role_suffix,
                    current_suffix,
                    w = label_w
                ));
            }
            if let Some(kids) = children.get(r.id.as_str()) {
                let mut kids = kids.clone();
                kids.sort_by(by_label);
                for &k in kids.iter().rev() {
                    stack.push((k, depth + 1));
                }
            }
        }
    }
    out
}

fn format_description_suffix(description: &str) -> String {
    if description.is_empty() {
        String::new()
    } else {
        format!(" - {}", description)
    }
}
