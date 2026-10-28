use super::BrowserRuntime;
use codeterm_browser::{
    snapshot::{parse_full_ax_tree, query},
    PageChannel,
};
use serde_json::json;

const BEGIN: &str = r#"(() => {
 const key = Symbol.for('domios.browser.capture');
 let state = window[key];
 if (!state) {
   state = { epoch: 0, document };
   state.observer = new MutationObserver(() => state.epoch++);
   state.observer.observe(document, {subtree:true, childList:true, attributes:true, characterData:true});
   for (const event of ['input','pointerdown','keydown']) document.addEventListener(event, () => state.epoch++, true);
   window[key] = state;
 }
 if (state.observer.takeRecords().length) state.epoch++;
 return state.epoch;
})()"#;

const ACT: &str = r#"function(epoch, action, text) {
 const state = window[Symbol.for('domios.browser.capture')];
 if (!state || state.document !== document || !this.isConnected) return 'stale';
 if (state.observer.takeRecords().length) state.epoch++;
 if (state.epoch !== epoch) return 'stale';
 if (this.disabled || this.readOnly || this.getAttribute('aria-disabled') === 'true') return 'disabled';
 if (!this.getClientRects().length) return 'hidden';
 if (action === 'click') { this.click(); return 'performed'; }
 const proto = this instanceof HTMLInputElement ? HTMLInputElement.prototype : this instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : null;
 const setter = proto && Object.getOwnPropertyDescriptor(proto, 'value').set;
 if (!setter) return 'unsupported_input';
 this.focus(); setter.call(this, text);
 this.dispatchEvent(new Event('input', {bubbles:true}));
 this.dispatchEvent(new Event('change', {bubbles:true}));
 return 'performed';
}"#;

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
        let args = parse(args, fill)?;
        let epoch = self.evaluate(page, BEGIN, false).await?;
        let capture = self.shared.begin_snapshot(page.session_id())?;
        self.send(page, "Accessibility.enable", json!({})).await?;
        let tree = self
            .send(page, "Accessibility.getFullAXTree", json!({}))
            .await?;
        let nodes = query::scoped(&parse_full_ax_tree(&tree)?, &args.within)?;
        let target = query::unique(&nodes, &args.role, Some(&args.name))?;
        let backend = target
            .backend_dom_node_id
            .ok_or("stale: missing DOM node")?;
        self.shared
            .finish_snapshot(&capture, Default::default())
            .map_err(|_| "stale: page changed during capture")?;
        let object = self
            .send(page, "DOM.resolveNode", json!({"backendNodeId":backend}))
            .await?;
        let id = object
            .pointer("/object/objectId")
            .and_then(|v| v.as_str())
            .ok_or("stale: detached element")?;
        let result = self.send(page,"Runtime.callFunctionOn",json!({"objectId":id,"functionDeclaration":ACT,"arguments":[{"value":epoch},{"value":if fill {"fill"} else {"click"}},{"value":args.text}],"returnByValue":true})).await;
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId":id}))
            .await;
        self.shared.invalidate_snapshot();
        let result = result.map_err(|_| {
            "action_unconfirmed: action may have occurred; inspect before continuing"
        })?;
        match result.pointer("/result/value").and_then(|v| v.as_str()) {
            Some("performed") => {
                Ok("action performed; re-snapshot to prove the resulting state".into())
            }
            Some(reason) => Err(format!("{reason}: atomic action refused")),
            None => Err("action_unconfirmed: inspect before continuing; never replay".into()),
        }
    }
}

#[cfg(test)]
mod tests;
