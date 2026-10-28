use super::{AxNode, AxValue};
use std::collections::{HashMap, HashSet};

pub fn unique<'a>(
    nodes: &'a [AxNode],
    role: &str,
    name: Option<&str>,
) -> Result<&'a AxNode, String> {
    let mut matches = nodes.iter().filter(|n| {
        !n.ignored
            && n.role.as_ref().map(AxValue::as_str) == Some(role)
            && name.is_none_or(|name| {
                n.name
                    .as_ref()
                    .map(AxValue::as_str)
                    .unwrap_or("")
                    .eq_ignore_ascii_case(name)
            })
    });
    let first = matches.next().ok_or("not_found: no matching element")?;
    if matches.next().is_some() {
        return Err("ambiguous: multiple matching elements".into());
    }
    Ok(first)
}

pub fn scoped(nodes: &[AxNode], scope: &str) -> Result<Vec<AxNode>, String> {
    let (role, name) = match scope {
        "dialog" => ("dialog", None),
        "sidebar" => ("complementary", None),
        "composer" => ("form", Some("composer")),
        _ => {
            let (role, name) = scope
                .split_once(':')
                .ok_or("invalid_scope: expected role:name")?;
            (role, Some(name))
        }
    };
    let root = unique(nodes, role, name)?;
    let by_id: HashMap<_, _> = nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    while let Some(node) = stack.pop() {
        if !seen.insert(node.node_id.as_str()) {
            continue;
        }
        out.push(node.clone());
        for id in node.child_ids.iter().rev() {
            if let Some(child) = by_id.get(id.as_str()) {
                stack.push(*child);
            }
        }
    }
    if let Some(root) = out.first_mut() {
        root.parent_id = None;
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
