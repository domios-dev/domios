use std::collections::VecDeque;

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

use super::grab::Rect;
use super::BrowserRuntime;
use crate::service::surface::runtime::{SurfaceCallCtx, SurfaceCallOutcome};

const MAX_SIDE_CSS: f64 = 8192.0;
const MAX_ORIGIN_CSS: f64 = 1_000_000.0;
const THUMB_SIDE_CSS: f64 = 64.0;
const THUMB_QUALITY: u8 = 60;
const PENDING_SNIPS: usize = 4;
pub(crate) const SNIP_COMMENT_MAX: usize = 8000;
const TITLE_MAX: usize = 200;
const URL_MAX: usize = 2048;

/// A region of the page in document CSS px, as the viewer measured it on the stream.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub(crate) struct SnipArgs {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SnipSendArgs {
    pub id: String,
    #[serde(default)]
    pub comment: String,
    /// False when the user removed the image chip: the context goes out without a file.
    #[serde(default = "attach_by_default")]
    pub image: bool,
}

fn attach_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SnipIdArgs {
    pub id: String,
}

/// Where the area sat when it was captured; viewport and scroll come from `Page.getLayoutMetrics`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SnipFacts {
    pub url: String,
    pub title: String,
    pub viewport: (f64, f64),
    pub scroll: (f64, f64),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Snip {
    pub id: String,
    pub rect: Rect,
    pub png: String,
    pub facts: SnipFacts,
}

/// Captures live in memory until sent, so a cancelled snip never touches the pad.
#[derive(Debug, Default)]
pub(crate) struct SnipStore {
    seq: u64,
    pending: VecDeque<Snip>,
}

impl SnipStore {
    pub(crate) fn keep(&mut self, rect: Rect, png: String, facts: SnipFacts) -> String {
        self.seq += 1;
        let id = format!("s{}", self.seq);
        self.pending.push_back(Snip {
            id: id.clone(),
            rect,
            png,
            facts,
        });
        while self.pending.len() > PENDING_SNIPS {
            self.pending.pop_front();
        }
        id
    }

    pub(crate) fn take(&mut self, id: &str) -> Option<Snip> {
        let index = self.pending.iter().position(|snip| snip.id == id)?;
        self.pending.remove(index)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.pending.len()
    }
}

/// The clip rounded to whole CSS px; rejects anything not finite, empty, negative or oversized.
pub(crate) fn snip_rect(args: &SnipArgs) -> Result<Rect, String> {
    let values = [args.x, args.y, args.w, args.h];
    if values.iter().any(|value| !value.is_finite()) {
        return Err("snip.capture: the area must be finite".into());
    }
    let rect = Rect {
        x: args.x.round(),
        y: args.y.round(),
        width: args.w.round(),
        height: args.h.round(),
    };
    if rect.x < 0.0 || rect.y < 0.0 || rect.x > MAX_ORIGIN_CSS || rect.y > MAX_ORIGIN_CSS {
        return Err("snip.capture: the area starts outside the page".into());
    }
    if rect.width < 1.0 || rect.height < 1.0 {
        return Err("snip.capture: the area is empty".into());
    }
    if rect.width > MAX_SIDE_CSS || rect.height > MAX_SIDE_CSS {
        return Err(format!(
            "snip.capture: the area is larger than {MAX_SIDE_CSS} px"
        ));
    }
    Ok(rect)
}

fn clip(rect: &Rect, scale: f64) -> Value {
    json!({ "x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height, "scale": scale })
}

/// Full resolution: clip scale 1 keeps the emulated device pixel ratio.
pub(crate) fn capture_params(rect: &Rect) -> Value {
    json!({ "format": "png", "clip": clip(rect, 1.0), "captureBeyondViewport": false })
}

pub(crate) fn thumbnail_params(rect: &Rect) -> Value {
    let longest = rect.width.max(rect.height).max(1.0);
    let scale = (THUMB_SIDE_CSS / longest).min(1.0);
    json!({
        "format": "jpeg",
        "quality": THUMB_QUALITY,
        "clip": clip(rect, scale),
        "captureBeyondViewport": false,
    })
}

/// Pixel size from the PNG IHDR without decoding the image.
pub(crate) fn png_size(base64: &str) -> Option<(u32, u32)> {
    let head = base64.get(..32)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(head)
        .ok()?;
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let read = |at: usize| Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    Some((read(16)?, read(20)?))
}

/// Viewport size and scroll offset from `Page.getLayoutMetrics`, visual viewport first.
pub(crate) fn layout_facts(metrics: &Value) -> ((f64, f64), (f64, f64)) {
    let viewport = metrics
        .get("cssVisualViewport")
        .or_else(|| metrics.get("cssLayoutViewport"));
    let number = |field: &str| {
        viewport
            .and_then(|viewport| viewport.get(field))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    };
    (
        (number("clientWidth"), number("clientHeight")),
        (number("pageX"), number("pageY")),
    )
}

fn one_line(text: &str, max: usize) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect();
    line.trim().to_string()
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The chat's image attachment tag, so the owner's transcript shows the crop.
pub(crate) fn attachment_marker(path: &str) -> String {
    format!(
        "<domios type=\"attachment\" kind=\"image\" path=\"{}\" />",
        escape_attribute(&one_line(path, URL_MAX))
    )
}

pub(crate) fn snip_markdown(snip: &Snip, comment: &str, image_path: Option<&str>) -> String {
    let facts = &snip.facts;
    let rect = &snip.rect;
    let comment: String = comment.trim().chars().take(SNIP_COMMENT_MAX).collect();
    let comment = if comment.is_empty() {
        "(no comment)".to_string()
    } else {
        comment
    };
    let mut lines = vec!["## Browser snip from the user".to_string()];
    if let Some(path) = image_path {
        lines.push(attachment_marker(path));
    }
    lines.push(format!(
        "**Page:** {} — `{}`",
        one_line(&facts.title, TITLE_MAX),
        one_line(&facts.url, URL_MAX)
    ));
    lines.push(format!(
        "**Viewport:** {}×{} · **Scroll:** ({}, {})",
        facts.viewport.0.round(),
        facts.viewport.1.round(),
        facts.scroll.0.round(),
        facts.scroll.1.round()
    ));
    lines.push(format!(
        "**Area:** {}×{} at ({}, {}) in page CSS px",
        rect.width, rect.height, rect.x, rect.y
    ));
    lines.push(format!("**User says:** {comment}"));
    lines.join("\n")
}

impl BrowserRuntime {
    /// `snip.capture`: read-only CDP calls, so the agent keeps driving the page.
    pub(super) async fn snip_capture(&self, args: SnipArgs) -> Result<Value, String> {
        let rect = snip_rect(&args)?;
        let page = self.ensure_page().await?;
        let shot = self
            .send(&page, "Page.captureScreenshot", capture_params(&rect))
            .await?;
        let png = shot
            .get("data")
            .and_then(Value::as_str)
            .ok_or("the snip returned no image")?
            .to_string();
        let thumbnail = self
            .send(&page, "Page.captureScreenshot", thumbnail_params(&rect))
            .await
            .ok()
            .and_then(|thumb| {
                thumb
                    .get("data")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .map(|data| format!("data:image/jpeg;base64,{data}"));
        let (viewport, scroll) = match self.send(&page, "Page.getLayoutMetrics", json!({})).await {
            Ok(metrics) => layout_facts(&metrics),
            Err(_) => ((0.0, 0.0), (0.0, 0.0)),
        };
        let state = self.shared.state();
        let (width, height) = png_size(&png).unwrap_or((0, 0));
        let facts = SnipFacts {
            url: state.url,
            title: state.title,
            viewport,
            scroll,
        };
        let id = self.snips.lock().keep(rect, png, facts);
        Ok(json!({ "id": id, "width": width, "height": height, "thumbnail": thumbnail }))
    }

    /// `snip.send`: the crop reaches the owner's pad only now, and only with the image kept.
    pub(super) async fn snip_send(
        &self,
        ctx: &SurfaceCallCtx,
        args: SnipSendArgs,
    ) -> Result<SurfaceCallOutcome, String> {
        let snip = self.snips.lock().take(&args.id).ok_or("unknown snip id")?;
        let image_path = if args.image {
            let name = format!("snip-{}-{}.png", snip.id, super::calls::now_ms());
            Some(
                self.save_evidence(ctx, name, snip.png.clone())
                    .await?
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        };
        let markdown = snip_markdown(&snip, &args.comment, image_path.as_deref());
        Ok(self.annotation_outcome(snip.id, markdown))
    }

    pub(super) fn snip_cancel(&self, id: &str) {
        self.snips.lock().take(id);
    }
}

#[cfg(test)]
mod tests;
