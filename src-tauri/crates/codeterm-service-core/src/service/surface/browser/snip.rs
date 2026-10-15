use std::collections::VecDeque;
use std::io::Cursor;

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat};
use serde::Deserialize;
use serde_json::{json, Value};

use super::grab::Rect;
use super::BrowserRuntime;
use crate::service::browser::screencast::FrameMetadata;
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

    pub fn take(&mut self, id: &str) -> Option<Snip> {
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

/// The whole viewport, unclipped: a clip or scale makes Chrome re-render the page, and the screencast shows it.
pub(crate) fn viewport_shot_params() -> Value {
    json!({ "format": "png" })
}

/// How the viewer mapped the stream to page px: the cast frame's metadata, inverted here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShotGeometry {
    pub device: (f64, f64),
    pub scroll: (f64, f64),
    pub page_scale: f64,
    pub offset_top: f64,
}

impl ShotGeometry {
    pub(crate) fn from_frame(metadata: &FrameMetadata) -> Option<Self> {
        (metadata.device_width > 0.0 && metadata.device_height > 0.0).then_some(Self {
            device: (metadata.device_width, metadata.device_height),
            scroll: (metadata.scroll_offset_x, metadata.scroll_offset_y),
            page_scale: metadata.page_scale_factor,
            offset_top: metadata.offset_top,
        })
    }

    pub(crate) fn from_layout(viewport: (f64, f64), scroll: (f64, f64)) -> Self {
        Self {
            device: viewport,
            scroll,
            page_scale: 1.0,
            offset_top: 0.0,
        }
    }

    /// Image px per device px; an unknown viewport assumes the shot is in device px.
    fn image_scale(&self, image: (u32, u32)) -> (f64, f64) {
        let axis = |device: f64, px: u32| {
            if device > 0.0 {
                px as f64 / device
            } else {
                1.0
            }
        };
        (axis(self.device.0, image.0), axis(self.device.1, image.1))
    }
}

/// Pixel box of the snip inside a viewport shot, clamped to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CropBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// `None` when nothing of the area is left inside the shot.
pub(crate) fn crop_box(rect: &Rect, geometry: &ShotGeometry, image: (u32, u32)) -> Option<CropBox> {
    let (sx, sy) = geometry.image_scale(image);
    let zoom = if geometry.page_scale > 0.0 {
        geometry.page_scale
    } else {
        1.0
    };
    let px = |page: f64| (page - geometry.scroll.0) * zoom * sx;
    let py = |page: f64| ((page - geometry.scroll.1) * zoom + geometry.offset_top) * sy;
    let clamp = |value: f64, limit: u32| value.round().clamp(0.0, limit as f64) as u32;
    let left = clamp(px(rect.x), image.0);
    let right = clamp(px(rect.x + rect.width), image.0);
    let top = clamp(py(rect.y), image.1);
    let bottom = clamp(py(rect.y + rect.height), image.1);
    if right <= left || bottom <= top {
        return None;
    }
    Some(CropBox {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SnipCut {
    pub png: String,
    pub width: u32,
    pub height: u32,
    pub thumbnail: Option<String>,
}

fn encode_png(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| format!("snip.capture: encode: {error}"))?;
    Ok(bytes.into_inner())
}

fn thumbnail_data_url(crop: &DynamicImage, side: u32) -> Option<String> {
    let side = side.max(1);
    let small = if crop.width().max(crop.height()) > side {
        crop.thumbnail(side, side)
    } else {
        crop.clone()
    };
    let mut bytes = Vec::new();
    DynamicImage::ImageRgb8(small.to_rgb8())
        .write_with_encoder(JpegEncoder::new_with_quality(&mut bytes, THUMB_QUALITY))
        .ok()?;
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/jpeg;base64,{data}"))
}

/// Crops the snip out of a base64 viewport PNG at full resolution, plus a small JPEG for the chip.
pub(crate) fn cut_snip(
    shot_base64: &str,
    rect: &Rect,
    geometry: &ShotGeometry,
) -> Result<SnipCut, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(shot_base64.as_bytes())
        .map_err(|error| format!("snip.capture: the screenshot is not base64: {error}"))?;
    let shot = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .map_err(|error| format!("snip.capture: decode: {error}"))?;
    let size = (shot.width(), shot.height());
    let area = crop_box(rect, geometry, size)
        .ok_or("snip.capture: the area is outside the visible page")?;
    let crop = shot.crop_imm(area.x, area.y, area.width, area.height);
    let (sx, sy) = geometry.image_scale(size);
    let thumb_side = (THUMB_SIDE_CSS * sx.max(sy)).round() as u32;
    Ok(SnipCut {
        png: base64::engine::general_purpose::STANDARD.encode(encode_png(&crop)?),
        width: area.width,
        height: area.height,
        thumbnail: thumbnail_data_url(&crop, thumb_side),
    })
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
        let (viewport, scroll) = match self.send(&page, "Page.getLayoutMetrics", json!({})).await {
            Ok(metrics) => layout_facts(&metrics),
            Err(_) => ((0.0, 0.0), (0.0, 0.0)),
        };
        let shot = {
            let _hold = self.host.hold_screencast_for_capture();
            self.send(&page, "Page.captureScreenshot", viewport_shot_params())
                .await?
        };
        let shot = shot
            .get("data")
            .and_then(Value::as_str)
            .ok_or("the snip returned no image")?
            .to_string();
        let geometry = self
            .host
            .latest_frame()
            .and_then(|frame| ShotGeometry::from_frame(&frame.metadata))
            .unwrap_or_else(|| ShotGeometry::from_layout(viewport, scroll));
        let cut_rect = rect.clone();
        let cut = tokio::task::spawn_blocking(move || cut_snip(&shot, &cut_rect, &geometry))
            .await
            .map_err(|error| format!("snip.capture: {error}"))??;
        let state = self.shared.state();
        let facts = SnipFacts {
            url: state.url,
            title: state.title,
            viewport,
            scroll,
        };
        let id = self.snips.lock().keep(rect, cut.png, facts);
        Ok(
            json!({ "id": id, "width": cut.width, "height": cut.height, "thumbnail": cut.thumbnail }),
        )
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
