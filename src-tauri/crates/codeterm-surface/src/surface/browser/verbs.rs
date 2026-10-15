use serde_json::{json, Value};

use super::heuristics::{activity_label, is_quiet_verb, IDLE};
use super::input::{self, InputArgs, NavAction};
use super::{cdp_error, BrowserRuntime, WAIT_DEADLINE};
use crate::surface::runtime::SurfaceCallCtx;
use codeterm_browser::snapshot::{
    build_snapshot, parse_full_ax_tree, resolve_target, DEFAULT_MAX_LINES,
};
use codeterm_browser::PageChannel;

pub const VERBS: &[&str] = &[
    "help",
    "open",
    "status",
    "profiles",
    "goto",
    "back",
    "forward",
    "reload",
    "snapshot",
    "click",
    "type",
    "press",
    "scroll",
    "text",
    "wait",
    "screenshot",
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
    let check = match wait {
        WaitFor::Text(text) => format!(
            "document.body && document.body.innerText.includes({})",
            Value::String(text.clone())
        ),
        WaitFor::Selector(selector) => format!(
            "!!document.querySelector({})",
            Value::String(selector.clone())
        ),
        WaitFor::Url(_) => return None,
    };
    let deadline = WAIT_DEADLINE.as_millis();
    let seconds = WAIT_DEADLINE.as_secs();
    Some(format!(
        "new Promise((resolve, reject) => {{ const ok = () => {check}; if (ok()) return resolve(true); \
         const obs = new MutationObserver(() => {{ if (ok()) {{ obs.disconnect(); resolve(true); }} }}); \
         obs.observe(document, {{ subtree: true, childList: true, characterData: true, attributes: true }}); \
         setTimeout(() => {{ obs.disconnect(); reject(new Error(\"not found within {seconds}s\")); }}, {deadline}); }})"
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

    async fn goto(&self, page: &PageChannel, url: &str) -> Result<String, String> {
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
            "snapshot" => {
                self.snapshot(page, parse_limit(args.first(), DEFAULT_MAX_LINES))
                    .await
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
            "type" => {
                let (text, locator) = usage_args(args, "type @eN|<role> <name> <text>")?
                    .split_last()
                    .ok_or("usage: type @eN|<role> <name> <text>")?;
                let target = self.resolve(locator)?;
                self.type_into(page, target.backend_node_id, text).await?;
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
            "wait" => self.wait(page, parse_wait(args)?).await,
            "screenshot" => {
                let full = args.iter().any(|arg| arg == "--full");
                let name = args.iter().find(|arg| !arg.starts_with("--"));
                let fallback = format!("shot-{}", unix_ms());
                let name = super::inbox::evidence_file_name(
                    name.map(String::as_str).unwrap_or(""),
                    &fallback,
                );
                let base64 = self.screenshot_base64(page, full).await?;
                let path = self.save_evidence(ctx, name, base64).await?;
                let path = path.to_string_lossy();
                Ok(format!("saved {path} (show it with @ds-image {path})"))
            }
            other => Err(format!("unknown verb \"{other}\"")),
        }
    }

    pub(super) fn resolve(
        &self,
        locator: &[String],
    ) -> Result<codeterm_browser::snapshot::ResolvedTarget, String> {
        let snapshot = self.shared.snapshot();
        let parts: Vec<&str> = locator.iter().map(String::as_str).collect();
        resolve_target(snapshot.as_ref(), &parts).map_err(|error| error.to_string())
    }

    async fn snapshot(&self, page: &PageChannel, limit: usize) -> Result<String, String> {
        let _ = self.send(page, "Accessibility.enable", json!({})).await;
        let tree = self
            .send(page, "Accessibility.getFullAXTree", json!({}))
            .await?;
        let snapshot = build_snapshot(&parse_full_ax_tree(&tree)?, limit);
        let text = snapshot.text.clone();
        self.shared.set_snapshot(snapshot);
        let state = self.shared.state();
        Ok(format!("page: {} — {}\n{text}", state.title, state.url))
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

    async fn evaluate(
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
                let mut navigated = page.subscribe_session(&["Page.frameNavigated"]);
                if self.shared.state().url.contains(fragment.as_str()) {
                    return Ok(format!("url already contains {fragment}"));
                }
                let matched = tokio::time::timeout(WAIT_DEADLINE, async {
                    while let Some(event) = navigated.recv().await {
                        let frame = event.params.get("frame");
                        let main = frame
                            .and_then(|f| f.get("parentId"))
                            .is_none_or(Value::is_null);
                        let url = frame
                            .and_then(|f| f.get("url"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if main && url.contains(fragment.as_str()) {
                            return Some(url.to_string());
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

fn unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
