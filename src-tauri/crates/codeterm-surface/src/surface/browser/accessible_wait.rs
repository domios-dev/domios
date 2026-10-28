use super::BrowserRuntime;
use codeterm_browser::{
    snapshot::{parse_full_ax_tree, query, AxNode},
    PageChannel,
};
use serde_json::{json, Value};
use std::time::Duration;

const BINDING: &str = "__domiosAXWake";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AccessibleWait {
    pub role: String,
    pub name: String,
    pub state: State,
    pub timeout: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Visible,
    Hidden,
    Enabled,
    Expanded,
}

pub(super) fn duration(raw: &str) -> Result<Duration, String> {
    let (number, scale) = if let Some(n) = raw.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = raw.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = raw.strip_suffix('m') {
        (n, 60000)
    } else {
        return Err("invalid_timeout: use ms, s or m".into());
    };
    let ms = number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(scale))
        .filter(|ms| (1..=120000).contains(ms))
        .ok_or("invalid_timeout: duration must be 1ms through 2m")?;
    Ok(Duration::from_millis(ms))
}

pub(super) fn parse(args: &[String]) -> Result<AccessibleWait, String> {
    let mut role = None;
    let mut name = None;
    let mut state = None;
    let mut timeout = Duration::from_secs(15);
    let mut i = 0;
    while i < args.len() {
        let flag = args[i].as_str();
        i += 1;
        let value = args.get(i).ok_or("invalid_wait: missing option value")?;
        match flag {
            "--role" if role.is_none() => role = Some(value.clone()),
            "--name" if name.is_none() => name = Some(value.clone()),
            "--state" if state.is_none() => {
                state = Some(match value.as_str() {
                    "visible" => State::Visible,
                    "hidden" => State::Hidden,
                    "enabled" => State::Enabled,
                    "expanded" => State::Expanded,
                    _ => return Err("invalid_wait_state".into()),
                })
            }
            "--timeout" => timeout = duration(value)?,
            _ => return Err("invalid_wait: unknown or duplicate option".into()),
        }
        i += 1;
    }
    Ok(AccessibleWait {
        role: role.ok_or("invalid_wait: --role required")?,
        name: name.ok_or("invalid_wait: --name required")?,
        state: state.ok_or("invalid_wait: --state required")?,
        timeout,
    })
}

pub(super) fn expanded(node: &AxNode) -> Option<bool> {
    node.properties
        .iter()
        .find(|p| p.name == "expanded")
        .and_then(|p| p.value.value.as_ref())
        .and_then(Value::as_bool)
}

impl BrowserRuntime {
    async fn accessible_condition(
        &self,
        page: &PageChannel,
        wait: &AccessibleWait,
    ) -> Result<bool, String> {
        let nodes = parse_full_ax_tree(
            &self
                .send(page, "Accessibility.getFullAXTree", json!({}))
                .await?,
        )?;
        let target = match query::unique(&nodes, &wait.role, Some(&wait.name)) {
            Ok(target) => target,
            Err(error) if error.starts_with("not_found:") => return Ok(wait.state == State::Hidden),
            Err(error) => return Err(error),
        };
        match wait.state {
            State::Expanded => Ok(expanded(target) == Some(true)),
            State::Enabled => Ok(!target
                .properties
                .iter()
                .any(|p| p.name == "disabled" && p.value.value == Some(Value::Bool(true)))),
            State::Visible | State::Hidden => {
                let Some(id) = target.backend_dom_node_id else {
                    return Ok(wait.state == State::Hidden);
                };
                let quads = self
                    .send(page, "DOM.getContentQuads", json!({"backendNodeId":id}))
                    .await?;
                let visible = super::input::quad_center(&quads).is_some();
                Ok(visible == (wait.state == State::Visible))
            }
        }
    }

    pub(super) async fn wait_accessible(
        &self,
        page: &PageChannel,
        wait: AccessibleWait,
    ) -> Result<String, String> {
        let mut events = page.subscribe_session(&[
            "Runtime.bindingCalled",
            "Page.frameNavigated",
            "Page.navigatedWithinDocument",
        ]);
        let id = uuid::Uuid::new_v4().simple().to_string();
        let deadline = tokio::time::Instant::now() + wait.timeout;
        let result = tokio::time::timeout_at(deadline, async {
            self.send(page, "Runtime.enable", json!({})).await?;
            self.send(page, "Accessibility.enable", json!({})).await?;
            self.send(page, "Runtime.addBinding", json!({"name":BINDING}))
                .await?;
            let expression = format!(
                "{}({}, {})",
                include_str!("accessible-wait.js"),
                json!(id),
                wait.timeout.as_millis()
            );
            self.evaluate(page, &expression, false).await?;
            loop {
                if self.accessible_condition(page, &wait).await? {
                    return Ok("accessible state confirmed".to_string());
                }
                loop {
                    let event = events
                        .recv()
                        .await
                        .ok_or("closed: page closed while waiting")?;
                    if event.method.starts_with("Page.") {
                        return Err("stale: page navigated while waiting".into());
                    }
                    if event.params.get("name").and_then(Value::as_str) == Some(BINDING) {
                        break;
                    }
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            Err("wait_timeout: accessible state was not observed before the deadline".into())
        });
        let cleanup = format!(
            "window[Symbol.for('domios.browser.wait')]?.waiters.delete({})",
            json!(id)
        );
        let _ = tokio::time::timeout_at(deadline, self.evaluate(page, &cleanup, false)).await;
        result
    }
}

#[cfg(test)]
mod tests;
