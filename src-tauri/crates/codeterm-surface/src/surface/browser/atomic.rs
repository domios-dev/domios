use super::BrowserRuntime;
use codeterm_browser::{
    snapshot::{parse_full_ax_tree, query, RefEntry},
    PageChannel,
};
use serde_json::json;

const BEGIN: &str = include_str!("atomic-begin.js");

const ACT: &str = include_str!("atomic-action.js");

#[derive(Debug, PartialEq, Eq)]
pub(super) struct AtomicArgs {
    pub within: String,
    pub role: String,
    pub name: String,
    pub text: Option<String>,
}

pub(super) fn parse(args: &[String], fill: bool) -> Result<AtomicArgs, String> {
    let mut within = None;
    let mut role = None;
    let mut name = None;
    let mut text = None;
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        if !flag.starts_with("--") && fill && text.is_none() {
            text = Some(args[i].clone());
            i += 1;
            continue;
        }
        i += 1;
        let value = args
            .get(i)
            .ok_or("invalid_locator: missing flag value")?
            .clone();
        let slot = match flag {
            "--within" => &mut within,
            "--role" => &mut role,
            "--name" => &mut name,
            "--text" if fill => &mut text,
            _ => return Err("invalid_locator: unknown option".into()),
        };
        if slot.replace(value).is_some() {
            return Err("invalid_locator: duplicate option".into());
        }
        i += 1;
    }
    if fill && text.is_none() {
        return Err("invalid_locator: fill requires text".into());
    }
    Ok(AtomicArgs {
        within: within.ok_or("invalid_locator: --within required")?,
        role: role.ok_or("invalid_locator: --role required")?,
        name: name.ok_or("invalid_locator: --name required")?,
        text,
    })
}

impl BrowserRuntime {
    pub(super) async fn atomic_action(
        &self,
        page: &PageChannel,
        args: &[String],
        fill: bool,
    ) -> Result<String, String> {
        self.atomic_request(page, parse(args, fill)?, fill, None)
            .await
    }

    pub(super) async fn disclosure_verb(
        &self,
        page: &PageChannel,
        args: &[String],
        expand: bool,
    ) -> Result<String, String> {
        if args.len() != 2 || !matches!(args[0].as_str(), "summary" | "DisclosureTriangle") {
            return Err("usage: expand|collapse summary <name>".into());
        }
        self.atomic_request(
            page,
            AtomicArgs {
                within: String::new(),
                role: "summary".into(),
                name: args[1].clone(),
                text: None,
            },
            false,
            Some(expand),
        )
        .await
    }

    async fn atomic_request(
        &self,
        page: &PageChannel,
        args: AtomicArgs,
        fill: bool,
        desired: Option<bool>,
    ) -> Result<String, String> {
        let _guard = self.action_gate.lock().await;
        let epoch = self.evaluate(page, BEGIN, false).await?;
        let capture = self
            .shared
            .begin_snapshot(page.session_id())
            .map_err(|_| "stale: browser session changed")?;
        self.send(page, "Accessibility.enable", json!({})).await?;
        let tree = self
            .send(page, "Accessibility.getFullAXTree", json!({}))
            .await?;
        let nodes = parse_full_ax_tree(&tree)?;
        let nodes = if args.within.is_empty() {
            nodes
        } else {
            query::scoped(&nodes, &args.within)?
        };
        let entries = nodes
            .iter()
            .filter(|node| {
                !node.ignored
                    && node
                        .role
                        .as_ref()
                        .and_then(|v| v.value.as_ref())
                        .and_then(|v| v.as_str())
                        .is_some_and(|role| {
                            role == args.role
                                || (args.role == "summary" && role == "DisclosureTriangle")
                        })
                    && node
                        .name
                        .as_ref()
                        .and_then(|v| v.value.as_ref())
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .eq_ignore_ascii_case(
                            &args.name.split_whitespace().collect::<Vec<_>>().join(" "),
                        )
            })
            .filter_map(|node| {
                node.backend_dom_node_id.map(|backend| RefEntry {
                    reference: format!("dom-{backend}"),
                    role: args.role.clone(),
                    name: args.name.clone(),
                    backend_node_id: backend,
                })
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            return Err("not_found: no matching element".into());
        }
        let visible = self.choose_visible(page, entries, fill, true).await?;
        let target = nodes
            .iter()
            .find(|node| node.backend_dom_node_id == Some(visible.backend_node_id))
            .ok_or("stale: missing target")?;
        let before = super::accessible_wait::expanded(target);
        if desired.is_some() && before.is_none() {
            return Err("unsupported: disclosure exposes no expanded state".into());
        }
        let backend = target
            .backend_dom_node_id
            .ok_or("stale: missing DOM node")?;
        self.shared
            .finish_snapshot(&capture, Default::default())
            .map_err(|_| "stale: page changed during capture")?;
        if desired.is_some() && desired == before {
            self.shared.invalidate_snapshot();
            return Ok("disclosure state confirmed; no action needed".into());
        }
        let object = self
            .send(page, "DOM.resolveNode", json!({"backendNodeId":backend}))
            .await?;
        let id = object
            .pointer("/object/objectId")
            .and_then(|v| v.as_str())
            .ok_or("stale: detached element")?;
        let result = self.complete_action(page, false, self.send(page,"Runtime.callFunctionOn",json!({"objectId":id,"functionDeclaration":ACT,"arguments":[{"value":epoch},{"value":if fill {"fill"} else {"click"}},{"value":args.text}],"returnByValue":true}))).await;
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId":id}))
            .await;
        self.shared.invalidate_snapshot();
        let (result, completion) = result?;
        match result.pointer("/result/value").and_then(|v| v.as_str()) {
            Some("performed") => {
                if let Some(desired) = desired {
                    let observed = self.send(page,"Accessibility.getFullAXTree",json!({})).await.map_err(|_| "state_unconfirmed: disclosure action occurred; inspect before continuing")?;
                    let nodes = parse_full_ax_tree(&observed)?;
                    let target = query::unique(&nodes, "summary", Some(&args.name))
                        .map_err(|_| "state_unconfirmed: disclosure changed after action")?;
                    if super::accessible_wait::expanded(target) != Some(desired) {
                        return Err("state_unconfirmed: disclosure action occurred; never replay without inspection".into());
                    }
                    Ok(if desired {
                        "expanded state confirmed"
                    } else {
                        "collapsed state confirmed"
                    }
                    .into())
                } else {
                    Ok(format!(
                        "{completion}; re-snapshot to prove the resulting state"
                    ))
                }
            }
            Some(reason) => Err(format!("{reason}: atomic action refused")),
            None => Err("action_unconfirmed: inspect before continuing; never replay".into()),
        }
    }
}

#[cfg(test)]
mod tests;
