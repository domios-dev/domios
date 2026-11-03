use super::BrowserRuntime;
use codeterm_browser::{
    snapshot::{RefEntry, ResolvedTarget, Snapshot},
    PageChannel,
};
use serde_json::json;

const INPUTS: &[&str] = &["textbox", "searchbox", "combobox", "spinbutton"];
const CLICKABLE: &[&str] = &[
    "link",
    "button",
    "tab",
    "menuitem",
    "checkbox",
    "switch",
    "radio",
    "DisclosureTriangle",
];

pub(super) fn candidates(snapshot: &Snapshot, args: &[String]) -> Result<Vec<RefEntry>, String> {
    if args.len() == 1 && args[0].starts_with('@') {
        let target = codeterm_browser::snapshot::resolve_target(Some(snapshot), &[&args[0]])
            .map_err(|error| error.to_string())?;
        return Ok(vec![RefEntry {
            reference: args[0][1..].into(),
            role: String::new(),
            name: target.label,
            backend_node_id: target.backend_node_id,
        }]);
    }
    let (role, name) = match args {
        [name] => (None, name.as_str()),
        [role, name] => (Some(role.as_str()), name.as_str()),
        _ => return Err("invalid_locator: expected @ref, name, or role name".into()),
    };
    let name = name.to_lowercase();
    let roles: Vec<_> = snapshot
        .index
        .iter()
        .filter(|entry| {
            role.is_none_or(|role| {
                entry.role == role || (role == "summary" && entry.role == "DisclosureTriangle")
            })
        })
        .collect();
    let exact = roles.iter().any(|entry| entry.name.to_lowercase() == name);
    let found: Vec<_> = roles
        .into_iter()
        .filter(|entry| {
            let actual = entry.name.to_lowercase();
            if exact {
                actual == name
            } else {
                actual.contains(&name)
            }
        })
        .cloned()
        .collect();
    if found.is_empty() {
        return Err("not_found: no matching target in the last snapshot; re-snapshot".into());
    }
    if found.len() > 128 {
        return Err("ambiguous: too many candidates; use a role or exact ref".into());
    }
    Ok(found)
}

pub(super) fn choose(
    mut visible: Vec<RefEntry>,
    input: bool,
    explicit: bool,
) -> Result<ResolvedTarget, String> {
    if !explicit {
        let preferred = if input { INPUTS } else { CLICKABLE };
        if visible
            .iter()
            .any(|entry| preferred.contains(&entry.role.as_str()))
        {
            visible.retain(|entry| preferred.contains(&entry.role.as_str()));
        }
    }
    match visible.as_slice() {
        [] => Err("hidden: no matching visible target; re-snapshot or expand the control".into()),
        [target] => Ok(ResolvedTarget {
            backend_node_id: target.backend_node_id,
            label: format!("@{}", target.reference),
        }),
        _ => Err(format!(
            "ambiguous: multiple visible targets; take a snapshot and use an exact ref: {}",
            visible
                .iter()
                .take(8)
                .map(|entry| if entry.reference.starts_with("dom-") {
                    format!("DOM node {} {}", entry.backend_node_id, entry.role)
                } else {
                    format!("@{} {}", entry.reference, entry.role)
                })
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

impl BrowserRuntime {
    pub(super) async fn visible_target(
        &self,
        page: &PageChannel,
        args: &[String],
        input: bool,
    ) -> Result<ResolvedTarget, String> {
        let snapshot = self
            .shared
            .snapshot()
            .ok_or("no snapshot yet — run `snapshot` first")?;
        let entries = candidates(&snapshot, args)?;
        let target = self
            .choose_visible(
                page,
                entries,
                input,
                args.len() == 2 || args[0].starts_with('@'),
            )
            .await?;
        let current = self.resolve(&[target.label.clone()])?;
        if current.backend_node_id != target.backend_node_id {
            return Err("stale: target changed; re-snapshot".into());
        }
        Ok(target)
    }

    pub(super) async fn choose_visible(
        &self,
        page: &PageChannel,
        entries: Vec<RefEntry>,
        input: bool,
        explicit: bool,
    ) -> Result<ResolvedTarget, String> {
        if entries.len() > 128 {
            return Err("ambiguous: too many candidates; narrow the locator".into());
        }
        let mut visible = Vec::new();
        for entry in entries {
            let object = self
                .send(
                    page,
                    "DOM.resolveNode",
                    json!({"backendNodeId":entry.backend_node_id}),
                )
                .await?;
            let id = object
                .pointer("/object/objectId")
                .and_then(|v| v.as_str())
                .ok_or("stale: target detached")?;
            let result = self.send(page, "Runtime.callFunctionOn", json!({"objectId":id,"functionDeclaration":include_str!("visible.js"),"returnByValue":true})).await;
            let _ = self
                .send(page, "Runtime.releaseObject", json!({"objectId":id}))
                .await;
            let result = result?;
            if result.pointer("/result/value").and_then(|v| v.as_bool()) == Some(true) {
                visible.push(entry);
            }
        }
        choose(visible, input, explicit)
    }
}

#[cfg(test)]
mod tests;
