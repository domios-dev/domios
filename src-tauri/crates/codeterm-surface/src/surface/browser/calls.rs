use serde::Deserialize;
use serde_json::{json, Value};

use super::grab::{self, Annotation, GrabTarget};
use super::input::{InputArgs, NavAction};
use super::snip::SnipIdArgs;
use super::BrowserRuntime;
use crate::surface::runtime::{SurfaceCallCtx, SurfaceCallOutcome, SurfaceEffect};
use codeterm_browser::viewport::ViewportRequest;
use codeterm_browser::PageChannel;

#[derive(Deserialize)]
struct ResizeArgs {
    width: f64,
    height: f64,
    #[serde(default = "unit_dpr")]
    dpr: f64,
}

fn unit_dpr() -> f64 {
    1.0
}

#[derive(Deserialize)]
struct NavArgs {
    action: String,
    #[serde(default)]
    target: Option<String>,
}

#[derive(Deserialize, Default)]
struct ScreenshotArgs {
    #[serde(default)]
    full: bool,
}

#[derive(Deserialize)]
struct ProfileArgs {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct PointArgs {
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct GrabSendArgs {
    id: String,
    #[serde(default)]
    comment: String,
    #[serde(default)]
    intent: Option<String>,
}

fn parse<T: for<'de> Deserialize<'de>>(method: &str, args: Value) -> Result<T, String> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args).map_err(|error| format!("{method}: invalid args: {error}"))
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

impl BrowserRuntime {
    /// The viewer methods of the stream surface contract (`POST /api/surfaces/{tab}/call`).
    pub(super) async fn view_call(
        &self,
        ctx: &SurfaceCallCtx,
        method: &str,
        args: Value,
    ) -> Result<SurfaceCallOutcome, String> {
        let value = match method {
            "resize" => {
                let size: ResizeArgs = parse(method, args)?;
                self.host
                    .set_viewport(ViewportRequest {
                        width: size.width,
                        height: size.height,
                        dpr: size.dpr,
                    })
                    .await?;
                Value::Null
            }
            "input" => {
                let input: InputArgs = parse(method, args)?;
                let page = self.ensure_page().await?;
                self.dispatch_input(&page, &input).await?;
                Value::Null
            }
            "nav" => {
                let nav: NavArgs = parse(method, args)?;
                let page = self.ensure_page().await?;
                self.navigate(&page, NavAction::parse(&nav.action)?, nav.target.as_deref())
                    .await?;
                Value::Null
            }
            "screenshot" => {
                let shot: ScreenshotArgs = parse(method, args)?;
                let page = self.ensure_page().await?;
                let base64 = self.screenshot_base64(&page, shot.full).await?;
                let path = self
                    .save_evidence(ctx, format!("shot-{}.png", now_ms()), base64.clone())
                    .await?;
                json!({ "base64": base64, "path": path.to_string_lossy() })
            }
            "profiles.list" => {
                self.refresh_profiles().await;
                self.profile_view()
            }
            "profiles.use" => {
                let profile: ProfileArgs = parse(method, args)?;
                self.use_profile(&profile.name).await?;
                self.profile_view()
            }
            "grab.hover" => {
                let point: PointArgs = parse(method, args)?;
                let page = self.ensure_page().await?;
                self.grab_hover(&page, point.x, point.y).await;
                Value::Null
            }
            "grab.cancel" => {
                if let Some(page) = self.host.page() {
                    self.grab_clear(&page).await;
                }
                Value::Null
            }
            "grab.select" => {
                let point: PointArgs = parse(method, args)?;
                let page = self.ensure_page().await?;
                let (target, png) = self.grab_collect(&page, point.x, point.y).await?;
                let id = self.next_grab_id();
                let label = grab::element_label(&target);
                let card = target.to_json();
                self.keep_grab(&id, target, png);
                json!({ "id": id, "label": label, "target": card })
            }
            "grab.send" => return self.grab_send(ctx, parse(method, args)?).await,
            "snip.capture" => self.snip_capture(parse(method, args)?).await?,
            "snip.cancel" => {
                let snip: SnipIdArgs = parse(method, args)?;
                self.snip_cancel(&snip.id);
                Value::Null
            }
            "snip.send" => return self.snip_send(ctx, parse(method, args)?).await,
            other => return Err(format!("unknown view method \"{other}\"")),
        };
        Ok(SurfaceCallOutcome::value(value))
    }

    async fn grab_send(
        &self,
        ctx: &SurfaceCallCtx,
        args: GrabSendArgs,
    ) -> Result<SurfaceCallOutcome, String> {
        let intent = grab::parse_intent(args.intent.as_deref())?;
        let (target, png) = self.take_grab(&args.id).ok_or("unknown grab id")?;
        let screenshot_path = match png {
            Some(png) => Some(
                self.save_evidence(ctx, format!("{}-{}.png", args.id, now_ms()), png)
                    .await?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None => None,
        };
        let markdown = grab::annotation_markdown(&Annotation {
            target: &target,
            intent,
            comment: &args.comment,
            screenshot_path: screenshot_path.as_deref(),
            sender: None,
        });
        Ok(self.annotation_outcome(args.id, markdown))
    }

    /// Kept for `inbox` first, so an undelivered annotation is never lost.
    pub(super) fn annotation_outcome(&self, id: String, markdown: String) -> SurfaceCallOutcome {
        self.inbox.lock().push(&id, &markdown);
        SurfaceCallOutcome {
            result: json!({ "markdown": markdown }),
            effects: vec![SurfaceEffect::DeliverAnnotation { id, markdown }],
        }
    }

    /// `annotate`: the same card the user's point-and-ask builds, for the element an agent names.
    pub(super) async fn annotate_verb(
        &self,
        ctx: &SurfaceCallCtx,
        args: &[String],
    ) -> Result<SurfaceCallOutcome, String> {
        let request = grab::parse_annotate(args)?;
        let page = self.ensure_page().await?;
        let target = self.resolve(&request.locator)?;
        let (target, png) = self.collect_node(&page, target.backend_node_id).await?;
        let id = self.next_grab_id();
        let screenshot_path = match png {
            Some(png) => Some(
                self.save_evidence(ctx, format!("{id}-{}.png", now_ms()), png)
                    .await?
                    .to_string_lossy()
                    .into_owned(),
            ),
            None => None,
        };
        let markdown = grab::annotation_markdown(&Annotation {
            target: &target,
            intent: request.intent,
            comment: &request.comment,
            screenshot_path: screenshot_path.as_deref(),
            sender: Some(ctx.actor.as_deref().unwrap_or("an agent")),
        });
        Ok(self.annotation_outcome(id, markdown))
    }

    async fn grab_setup(&self, page: &PageChannel) -> Result<(), String> {
        if !self.shared.claim_grab_setup() {
            return Ok(());
        }
        let enabled = async {
            self.send(page, "DOM.enable", json!({})).await?;
            self.send(page, "Overlay.enable", json!({})).await
        }
        .await;
        if let Err(error) = enabled {
            self.shared.reset_grab_setup();
            return Err(error);
        }
        let _ = self.send(page, "Accessibility.enable", json!({})).await;
        Ok(())
    }

    async fn node_at(&self, page: &PageChannel, x: f64, y: f64) -> Result<i64, String> {
        self.grab_setup(page).await?;
        let node = self
            .send(
                page,
                "DOM.getNodeForLocation",
                json!({
                    "x": x.round() as i64,
                    "y": y.round() as i64,
                    "includeUserAgentShadowDOM": false,
                    "ignorePointerEventsNone": true,
                }),
            )
            .await?;
        node.get("backendNodeId")
            .and_then(Value::as_i64)
            .ok_or_else(|| "no element at that point".to_string())
    }

    async fn grab_hover(&self, page: &PageChannel, x: f64, y: f64) {
        if let Ok(backend_node_id) = self.node_at(page, x, y).await {
            let _ = self
                .send(
                    page,
                    "Overlay.highlightNode",
                    json!({ "highlightConfig": grab::highlight_config(), "backendNodeId": backend_node_id }),
                )
                .await;
        }
    }

    async fn grab_clear(&self, page: &PageChannel) {
        if self.shared.grab_ready() {
            let _ = self.send(page, "Overlay.hideHighlight", json!({})).await;
        }
    }

    async fn grab_collect(
        &self,
        page: &PageChannel,
        x: f64,
        y: f64,
    ) -> Result<(GrabTarget, Option<String>), String> {
        let backend_node_id = self.node_at(page, x, y).await?;
        let collected = self.collect_node(page, backend_node_id).await;
        self.grab_clear(page).await;
        collected
    }

    async fn collect_node(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
    ) -> Result<(GrabTarget, Option<String>), String> {
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
        let collected = self
            .send(
                page,
                "Runtime.callFunctionOn",
                json!({ "objectId": object_id, "functionDeclaration": grab::COLLECT_IN_PAGE, "returnByValue": true }),
            )
            .await?;
        let raw = collected
            .pointer("/result/value")
            .and_then(Value::as_str)
            .ok_or("the element facts did not come back")?;
        let mut target: GrabTarget =
            serde_json::from_str(raw).map_err(|error| format!("element facts: {error}"))?;
        if let Ok(tree) = self
            .send(
                page,
                "Accessibility.getPartialAXTree",
                json!({ "backendNodeId": backend_node_id, "fetchRelatives": false }),
            )
            .await
        {
            target.accessibility = grab::accessibility_of(&tree);
        }
        let png = match grab::screenshot_clip(&target.rect_page) {
            Some(clip) => self
                .send(
                    page,
                    "Page.captureScreenshot",
                    json!({ "format": "png", "clip": clip, "captureBeyondViewport": true }),
                )
                .await
                .ok()
                .and_then(|shot| shot.get("data").and_then(Value::as_str).map(str::to_string)),
            None => None,
        };
        Ok((target, png))
    }
}
