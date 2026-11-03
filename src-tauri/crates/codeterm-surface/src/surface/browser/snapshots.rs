use super::BrowserRuntime;
use codeterm_browser::{
    snapshot::{
        build_snapshot_with_text, parse_full_ax_tree, query, sensitive, DEFAULT_MAX_LINES,
        MAX_LINES,
    },
    PageChannel,
};
use serde_json::json;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Options {
    pub json: bool,
    pub scope: Option<String>,
    pub actions: bool,
    pub limit: usize,
    pub with_text: bool,
    pub reference: Option<String>,
    pub depth: Option<usize>,
}

pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let mut out = Options {
        json: false,
        scope: None,
        actions: false,
        limit: DEFAULT_MAX_LINES,
        with_text: false,
        reference: None,
        depth: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--ref" if out.reference.is_none() => {
                i += 1;
                out.reference = Some(
                    args.get(i)
                        .filter(|s| s.starts_with('@'))
                        .ok_or("--ref requires a snapshot ref")?
                        .clone(),
                );
            }
            "--depth" if out.depth.is_none() => {
                i += 1;
                out.depth = Some(
                    args.get(i)
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|n| *n <= 64)
                        .ok_or("depth must be 0 to 64")?,
                );
            }
            "--json" => out.json = true,
            "--with-text" => out.with_text = true,
            "--actions-only" => out.actions = true,
            "--scope" => {
                i += 1;
                out.scope = Some(args.get(i).ok_or("--scope requires a value")?.clone());
            }
            "--max-items" => {
                i += 1;
                out.limit = args
                    .get(i)
                    .ok_or("--max-items requires a number")?
                    .parse::<usize>()
                    .map_err(|_| "invalid max-items")?;
                if out.limit == 0 {
                    return Err("max-items must be positive".into());
                }
            }
            value if !value.starts_with("--") => {
                out.limit = value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .unwrap_or(DEFAULT_MAX_LINES)
            }
            _ => return Err("unknown snapshot option".into()),
        }
        i += 1;
    }
    out.limit = out.limit.min(MAX_LINES);
    if out.scope.is_some() && out.reference.is_some() {
        return Err("choose --scope or --ref".into());
    }
    Ok(out)
}

impl BrowserRuntime {
    pub(super) async fn snapshot_verb(
        &self,
        page: &PageChannel,
        args: &[String],
    ) -> Result<String, String> {
        let options = parse(args)?;
        let backend = options
            .reference
            .as_ref()
            .map(|reference| {
                self.resolve(std::slice::from_ref(reference))
                    .map(|target| target.backend_node_id)
            })
            .transpose()?;
        let capture = self.shared.begin_snapshot(page.session_id())?;
        self.send(page, "Accessibility.enable", json!({})).await?;
        let mut tree = self
            .send(page, "Accessibility.getFullAXTree", json!({}))
            .await?;
        self.redact_value(&mut tree);
        let mut nodes = parse_full_ax_tree(&tree)?;
        let document = self
            .send(page, "DOM.getDocument", json!({"depth":-1,"pierce":true}))
            .await?;
        sensitive::redact(&mut nodes, &document)?;
        if let Some(scope) = &options.scope {
            nodes = query::scoped(&nodes, scope)?;
        }
        let (nodes, depth_truncated) = restrict(nodes, backend, options.depth)?;
        let mut snapshot = build_snapshot_with_text(
            &nodes,
            options.limit,
            &capture,
            options.actions,
            options.with_text,
        );
        snapshot.truncated |= depth_truncated;
        if depth_truncated && !options.json {
            snapshot.text.push_str("\n(truncated at requested depth)");
        }
        let state = self.shared.state();
        let result = if options.json {
            json!({"page":{"title":state.title.chars().take(200).collect::<String>(),"url":state.url.chars().take(2000).collect::<String>()},"scope":options.scope,"items":snapshot.items,"truncated":snapshot.truncated,"maxItems":options.limit}).to_string()
        } else {
            format!("page: {} — {}\n{}", state.title, state.url, snapshot.text)
        };
        self.shared.finish_snapshot(&capture, snapshot)?;
        Ok(result)
    }
}

fn restrict(
    nodes: Vec<codeterm_browser::snapshot::AxNode>,
    backend: Option<i64>,
    depth: Option<usize>,
) -> Result<(Vec<codeterm_browser::snapshot::AxNode>, bool), String> {
    use std::collections::{HashMap, HashSet};
    let by_id: HashMap<_, _> = nodes
        .iter()
        .map(|node| (node.node_id.as_str(), node))
        .collect();
    let roots: Vec<_> = if let Some(backend) = backend {
        vec![nodes
            .iter()
            .find(|node| node.backend_dom_node_id == Some(backend))
            .ok_or("stale: subtree ref no longer exists")?]
    } else {
        nodes
            .iter()
            .filter(|node| {
                node.parent_id
                    .as_deref()
                    .is_none_or(|id| !by_id.contains_key(id))
            })
            .collect()
    };
    let mut stack: Vec<_> = roots.iter().rev().map(|node| (*node, 0)).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut truncated = false;
    while let Some((node, level)) = stack.pop() {
        if !seen.insert(node.node_id.as_str()) {
            continue;
        }
        let mut copy = node.clone();
        if level == 0 {
            copy.parent_id = None;
        }
        if depth.is_some_and(|depth| level >= depth) {
            truncated |= !copy.child_ids.is_empty();
            copy.child_ids.clear();
        } else {
            for id in node.child_ids.iter().rev() {
                if let Some(child) = by_id.get(id.as_str()) {
                    stack.push((*child, level + 1));
                }
            }
        }
        out.push(copy);
    }
    Ok((out, truncated))
}

#[cfg(test)]
mod tests;
