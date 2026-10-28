use serde_json::{json, Value};

use super::heuristics::{activity_label, is_quiet_verb, IDLE};
use super::input::{self, InputArgs, NavAction};
use super::{cdp_error, BrowserRuntime, WAIT_DEADLINE};
use crate::surface::runtime::SurfaceCallCtx;
use codeterm_browser::PageChannel;

pub const VERBS: &[&str] = &[
    "help",
    "open",
    "status",
    "profiles",
    "profile",
    "connect-secret",
    "file-picker",
    "goto",
    "back",
    "forward",
    "reload",
    "snapshot",
    "click",
    "fill",
    "type",
    "type-secret",
    "press",
    "scroll",
    "text",
    "wait",
    "screenshot",
    "viewport",
    "inbox",
    "annotate",
    "endpoint",
    "close",
];

const DEFAULT_TEXT_LIMIT: usize = 4000;
const DEFAULT_SCROLL: f64 = 800.0;

/// `open [url] [--profile NAME]`: the url and the requested profile, if any.
pub fn parse_open(args: &[String]) -> (Option<String>, Option<String>) {
    let mut url = None;
    let mut profile = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--profile" => profile = rest.next().cloned(),
            "--url" => url = rest.next().cloned(),
            _ if url.is_none() => url = Some(arg.clone()),
            _ => {}
        }
    }
    (url, profile)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitFor {
    Text(String),
    Url(String),
    Selector(String),
}

pub fn parse_wait(args: &[String]) -> Result<WaitFor, String> {
    const USAGE: &str = "usage: wait --text T | --url U | --selector S";
    let value = args.get(1..).map(|rest| rest.join(" ")).unwrap_or_default();
    if value.is_empty() {
        return Err(USAGE.into());
    }
    match args.first().map(String::as_str) {
        Some("--text") => Ok(WaitFor::Text(value)),
        Some("--url") => Ok(WaitFor::Url(value)),
        Some("--selector") => Ok(WaitFor::Selector(value)),
        _ => Err(USAGE.into()),
    }
}

/// An in-page MutationObserver resolves the moment the condition holds; its timer only means failure.
pub fn wait_expression(wait: &WaitFor) -> Option<String> {
    let (kind, value) = match wait {
        WaitFor::Text(text) => ("text", text),
        WaitFor::Selector(selector) => ("selector", selector),
        WaitFor::Url(_) => return None,
    };
    Some(format!(
        "{}({}, {}, {})",
        include_str!("wait.js"),
        Value::String(kind.into()),
        Value::String(value.clone()),
        WAIT_DEADLINE.as_millis(),
    ))
}

fn parse_limit(raw: Option<&String>, default: usize) -> usize {
    raw.and_then(|raw| raw.parse::<usize>().ok())
        .filter(|limit| *limit > 0)
        .unwrap_or(default)
}

fn usage_args<'a>(args: &'a [String], usage: &str) -> Result<&'a [String], String> {
    if args.is_empty() {
        Err(format!("usage: {usage}"))
    } else {
        Ok(args)
    }
}

impl BrowserRuntime {
    /// Agent verbs with the POC's semantics; `help` and `close` are answered by core.
    pub(super) async fn run_verb(
        &self,
        ctx: &SurfaceCallCtx,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        if !VERBS.contains(&verb) {
            return Err(format!(
                "unknown verb \"{verb}\". Verbs: {}",
                VERBS.join(", ")
            ));
        }
        let quiet = is_quiet_verb(verb);
        if !quiet {
            self.shared
                .set_activity(&activity_label(verb, args), ctx.actor.as_deref());
        }
        let outcome = self.verb(ctx, verb, args).await;
        if !quiet {
            self.shared.set_activity(IDLE, None);
        }
        outcome
    }

    pub(super) async fn run_annotate(
        &self,
        ctx: &SurfaceCallCtx,
        args: &[String],
    ) -> Result<crate::surface::runtime::SurfaceCallOutcome, String> {
        self.shared
            .set_activity(&activity_label("annotate", args), ctx.actor.as_deref());
        let outcome = self.annotate_verb(ctx, args).await;
        self.shared.set_activity(IDLE, None);
        outcome
    }

    async fn verb(
        &self,
        ctx: &SurfaceCallCtx,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        match verb {
            "status" => Ok(self.status_text()),
            "profile" => self.profile_verb(args).await,
            "connect-secret" => self.connect_session(args).await,
            "file-picker" => Err("unsupported: native file picker driving is unavailable; use Take control to choose files".into()),
            "profiles" => {
                self.refresh_profiles().await;
                let state = self.shared.state();
                let mut available = vec!["clean".to_string()];
                available.extend(state.profiles);
                Ok(format!(
                    "current: {}\navailable: {}",
                    state.profile,
                    available.join(", ")
                ))
            }
            "inbox" => Ok(self
                .inbox
                .lock()
                .take(args.first().is_some_and(|a| a == "all"))),
            "endpoint" => Ok(self.host.endpoint().unwrap_or_default()),
            "open" => self.open_verb(args).await,
            "help" | "close" | "annotate" => Err(format!("{verb} is answered by the surface host")),
            _ => {
                let page = self.ensure_page().await?;
                self.page_verb(ctx, &page, verb, args).await
            }
        }
    }

    fn status_text(&self) -> String {
        let state = self.shared.state();
        let title = if state.title.is_empty() {
            "(untitled)"
        } else {
            state.title.as_str()
        };
        [
            format!("owner: {}", state.owner),
            format!("profile: {}", state.profile),
            format!("page: {title} — {}", state.url),
            format!("needs human: {}", state.needs_human),
        ]
        .join("\n")
    }

    async fn open_verb(&self, args: &[String]) -> Result<String, String> {
        if args
            .iter()
            .any(|a| matches!(a.as_str(), "--origin" | "--ephemeral" | "--expect-build"))
        {
            return self.open_session(args).await;
        }
        let (url, profile) = parse_open(args);
        match profile {
            Some(profile) => self.use_profile(&profile).await?,
            None => {
                self.ensure_page().await?;
            }
        }
        if let Some(url) = url {
            let page = self.ensure_page().await?;
            self.goto(&page, &url).await?;
        }
        Ok(self.status_text())
    }

    pub(super) async fn goto(&self, page: &PageChannel, url: &str) -> Result<String, String> {
        let mut loaded = page.subscribe_session(&["Page.loadEventFired"]);
        self.navigate(page, NavAction::Goto, Some(url)).await?;
        let _ = tokio::time::timeout(WAIT_DEADLINE, loaded.recv()).await;
        let state = self.shared.state();
        Ok(format!("{} — {}", state.url, state.title))
    }

    async fn history_step(&self, page: &PageChannel, action: NavAction) -> Result<String, String> {
        let events = match action {
            NavAction::Reload => vec!["Page.loadEventFired"],
            _ => vec!["Page.frameNavigated", "Page.navigatedWithinDocument"],
        };
        let mut moved = page.subscribe_session(&events);
        self.navigate(page, action, None).await?;
        let _ = tokio::time::timeout(WAIT_DEADLINE, moved.recv()).await;
        Ok(self.shared.state().url)
    }

    async fn page_verb(
        &self,
        ctx: &SurfaceCallCtx,
        page: &PageChannel,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        match verb {
            "goto" => {
                let url = usage_args(args, "goto <url>")?.join(" ");
                self.goto(page, &url).await
            }
            "back" => self.history_step(page, NavAction::Back).await,
            "forward" => self.history_step(page, NavAction::Forward).await,
            "reload" => self.history_step(page, NavAction::Reload).await,
            "snapshot" => self.snapshot_verb(page, args).await,
            "click" | "fill" if verb == "fill" || args.iter().any(|a| a == "--within") => {
                self.atomic_action(page, args, verb == "fill").await
            }
            "click" => {
                let target = self.resolve(args)?;
                self.click_node(page, target.backend_node_id).await?;
                Ok(format!(
                    "clicked {} → {} (re-snapshot before the next action)",
                    target.label,
                    self.shared.state().url
                ))
            }
            "type" | "type-secret" => {
                let (text, locator) = usage_args(args, "type @eN|<role> <name> <text>")?
                    .split_last()
                    .ok_or("usage: type @eN|<role> <name> <text>")?;
                let target = self.resolve(locator)?;
                self.type_into(page, target.backend_node_id, text, verb == "type-secret")
                    .await?;
                Ok(format!("typed into {}", target.label))
            }
            "press" => {
                let (key, mods) = usage_args(args, "press <key> [shift|ctrl|alt|meta]")?
                    .split_first()
                    .ok_or("usage: press <key>")?;
                self.dispatch_input(page, &InputArgs::key(key, mods))
                    .await?;
                Ok(format!("pressed {key}"))
            }
            "scroll" => {
                let dy = args
                    .first()
                    .and_then(|raw| raw.parse::<f64>().ok())
                    .filter(|dy| dy.is_finite() && *dy != 0.0)
                    .unwrap_or(DEFAULT_SCROLL);
                self.evaluate(page, &format!("window.scrollBy(0, {dy})"), false)
                    .await?;
                Ok("scrolled (re-snapshot to see new content)".into())
            }
            "text" => {
                let limit = parse_limit(args.first(), DEFAULT_TEXT_LIMIT);
                let expression =
                    format!("document.body ? document.body.innerText.slice(0, {limit}) : \"\"");
                let value = self.evaluate(page, &expression, false).await?;
                Ok(value
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .unwrap_or("(empty page)")
                    .to_string())
            }
            "wait" if args.iter().any(|a| a == "--role") => {
                self.wait_accessible(page, super::accessible_wait::parse(args)?)
                    .await
            }
            "wait" => self.wait(page, parse_wait(args)?).await,
            "viewport" => self.viewport_verb(args).await,
            "screenshot" => self.screenshot_verb(ctx, args).await,
            other => Err(format!("unknown verb \"{other}\"")),
        }
    }

    pub(super) fn resolve(
        &self,
        locator: &[String],
    ) -> Result<codeterm_browser::snapshot::ResolvedTarget, String> {
        let parts: Vec<&str> = locator.iter().map(String::as_str).collect();
        self.shared.resolve(&parts)
    }

    async fn click_node(&self, page: &PageChannel, backend_node_id: i64) -> Result<(), String> {
        let node = json!({ "backendNodeId": backend_node_id });
        let _ = self
            .send(page, "DOM.scrollIntoViewIfNeeded", node.clone())
            .await;
        let quads = self.send(page, "DOM.getContentQuads", node).await?;
        let (x, y) =
            input::quad_center(&quads).ok_or("element has no visible box (hidden or zero-size)")?;
        self.dispatch_input(page, &InputArgs::click(x, y)).await
    }

    async fn type_into(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
        text: &str,
        secret: bool,
    ) -> Result<(), String> {
        let resolved = self
            .send(
                page,
                "DOM.resolveNode",
                json!({ "backendNodeId": backend_node_id }),
            )
            .await?;
        let object_id = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or("the element could not be resolved")?;
        if secret {
            let masked = self.send(page, "Runtime.callFunctionOn", json!({
                "objectId": object_id,
                "functionDeclaration": "function(){ if (!(this instanceof HTMLInputElement)) return false; this.type='password'; return this.type==='password'; }",
                "returnByValue": true,
            })).await?;
            if masked.pointer("/result/value").and_then(Value::as_bool) != Some(true) {
                return Err("Secret typing requires an input that can be masked".into());
            }
        }
        self.send(
            page,
            "Runtime.callFunctionOn",
            json!({
                "objectId": object_id,
                "functionDeclaration": "function(){ this.focus(); if ('value' in this) { this.value=''; this.dispatchEvent(new Event('input',{bubbles:true})); } }",
            }),
        )
        .await?;
        self.send(page, "Input.insertText", json!({ "text": text }))
            .await
            .map(|_| ())
    }

    pub(super) async fn evaluate(
        &self,
        page: &PageChannel,
        expression: &str,
        await_promise: bool,
    ) -> Result<Value, String> {
        let result = page
            .send(
                "Runtime.evaluate",
                json!({ "expression": expression, "awaitPromise": await_promise, "returnByValue": true }),
            )
            .await
            .map_err(cdp_error)?;
        if let Some(details) = result.get("exceptionDetails") {
            let description = details
                .pointer("/exception/description")
                .and_then(Value::as_str)
                .and_then(|text| text.lines().next())
                .unwrap_or("failed");
            return Err(description.to_string());
        }
        Ok(result
            .pointer("/result/value")
            .cloned()
            .unwrap_or(Value::Null))
    }

    async fn wait(&self, page: &PageChannel, wait: WaitFor) -> Result<String, String> {
        match &wait {
            WaitFor::Url(fragment) => {
                let mut navigated = page
                    .subscribe_session(&["Page.frameNavigated", "Page.navigatedWithinDocument"]);
                if self.shared.state().url.contains(fragment.as_str()) {
                    return Ok(format!("url already contains {fragment}"));
                }
                let matched = tokio::time::timeout(WAIT_DEADLINE, async {
                    while let Some(event) = navigated.recv().await {
                        let update = super::page::page_update(
                            &event.method,
                            &event.params,
                            page.target_id(),
                        );
                        if let Some(
                            super::page::PageUpdate::Navigated { url }
                            | super::page::PageUpdate::SameDocument { url },
                        ) = update
                        {
                            if url.contains(fragment.as_str()) {
                                return Some(url);
                            }
                        }
                    }
                    None
                })
                .await;
                match matched {
                    Ok(Some(url)) => Ok(format!("url: {url}")),
                    Ok(None) => Err("the page closed while waiting".into()),
                    Err(_) => Err(format!(
                        "url containing \"{fragment}\" did not happen within {}s",
                        WAIT_DEADLINE.as_secs()
                    )),
                }
            }
            WaitFor::Text(value) | WaitFor::Selector(value) => {
                let expression = wait_expression(&wait).ok_or("unsupported wait")?;
                let (flag, found) = match &wait {
                    WaitFor::Text(_) => ("--text", format!("text \"{value}\"")),
                    _ => ("--selector", format!("selector {value}")),
                };
                self.evaluate(page, &expression, true)
                    .await
                    .map_err(|error| format!("{flag} {value}: {error}"))?;
                Ok(format!("found {found}"))
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "verbs/secret_tests.rs"]
mod secret_tests;
