use std::future::Future;
use std::time::Duration;

use base64::Engine;
use codeterm_browser::PageChannel;
use image::ImageFormat;
use serde::Deserialize;
use serde_json::json;

use super::BrowserRuntime;

const RENDER_STATE_JS: &str = include_str!("render-state.js");
const FRAME_COMMIT_JS: &str =
    "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true))))";
const FRAME_COMMIT_DEADLINE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CapturePixels {
    Rendered,
    Uniform {
        rgba: [u8; 4],
        width: u32,
        height: u32,
    },
    Empty,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageRenderState {
    #[serde(default)]
    pub visibility_state: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub has_focus: bool,
    #[serde(default)]
    pub has_text: bool,
    #[serde(default)]
    pub visible_elements: usize,
    #[serde(default)]
    pub viewport_width: f64,
    #[serde(default)]
    pub viewport_height: f64,
}

pub(super) fn inspect_png_bytes(bytes: &[u8]) -> Result<CapturePixels, String> {
    let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)
        .map_err(|error| format!("surface not rendering: invalid PNG capture ({error})"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return Ok(CapturePixels::Empty);
    }
    let (pixels, _) = rgba.as_raw().as_chunks::<4>();
    let Some((first, rest)) = pixels.split_first() else {
        return Ok(CapturePixels::Empty);
    };
    if rest.iter().any(|pixel| pixel != first) {
        return Ok(CapturePixels::Rendered);
    }
    Ok(CapturePixels::Uniform {
        rgba: *first,
        width,
        height,
    })
}

pub(super) fn inspect_png_base64(encoded: &str) -> Result<CapturePixels, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| format!("surface not rendering: screenshot is not base64 ({error})"))?;
    inspect_png_bytes(&bytes)
}

pub(super) fn classify_capture(
    pixels: CapturePixels,
    state: Option<&PageRenderState>,
    is_region_crop: bool,
) -> Result<(), String> {
    match pixels {
        CapturePixels::Rendered => Ok(()),
        CapturePixels::Empty => Err("surface not rendering: captured an empty image".into()),
        CapturePixels::Uniform {
            rgba,
            width,
            height,
        } => {
            let is_black_or_transparent =
                rgba[3] == 0 || (rgba[0] == 0 && rgba[1] == 0 && rgba[2] == 0);
            if let Some(state) = state {
                if state.hidden
                    || (!state.visibility_state.is_empty() && state.visibility_state != "visible")
                {
                    let vis = if state.visibility_state.is_empty() {
                        "hidden"
                    } else {
                        state.visibility_state.as_str()
                    };
                    return Err(format!(
                        "surface not rendering: document visibilityState is {vis} (captured blank {width}x{height} frame)"
                    ));
                }
                if state.viewport_width <= 0.0 || state.viewport_height <= 0.0 {
                    return Err(format!(
                        "surface not rendering: viewport is collapsed ({}x{})",
                        state.viewport_width, state.viewport_height
                    ));
                }
                let has_content = state.visible_elements > 0 || state.has_text;
                let focus_note = if state.has_focus { "" } else { ", unfocused" };
                if is_black_or_transparent && has_content {
                    return Err(format!(
                        "surface not rendering: captured an all-black {width}x{height} frame while page has visible content (visibilityState={}{focus_note}); target compositor surface may be stuck — close and reopen the browser surface",
                        if state.visibility_state.is_empty() {
                            "unknown"
                        } else {
                            state.visibility_state.as_str()
                        }
                    ));
                }
                if !is_region_crop && state.visible_elements > 0 && state.has_text {
                    return Err(format!(
                        "surface not rendering: captured a uniform blank {width}x{height} frame (rgb({},{},{})) while page has visible content (visibilityState={}{focus_note}); target compositor surface may be stuck — close and reopen the browser surface",
                        rgba[0],
                        rgba[1],
                        rgba[2],
                        if state.visibility_state.is_empty() {
                            "unknown"
                        } else {
                            state.visibility_state.as_str()
                        }
                    ));
                }
                return Ok(());
            }
            if is_black_or_transparent && !is_region_crop {
                return Err(format!(
                    "surface not rendering: captured an all-black {width}x{height} frame (render state unavailable)"
                ));
            }
            Ok(())
        }
    }
}

impl BrowserRuntime {
    async fn probe_render_state(&self, page: &PageChannel) -> Option<PageRenderState> {
        let value = self.evaluate(page, RENDER_STATE_JS, false).await.ok()?;
        serde_json::from_value(value).ok()
    }

    async fn wake_surface_for_capture(&self, page: &PageChannel) -> bool {
        let _ = self.send(page, "Page.bringToFront", json!({})).await;
        let focus_enabled = self
            .send(
                page,
                "Emulation.setFocusEmulationEnabled",
                json!({ "enabled": true }),
            )
            .await
            .is_ok();
        let _ = tokio::time::timeout(
            FRAME_COMMIT_DEADLINE,
            self.evaluate(page, FRAME_COMMIT_JS, true),
        )
        .await;
        focus_enabled
    }

    async fn restore_focus_emulation(&self, page: &PageChannel, focus_enabled: bool) {
        if focus_enabled {
            let _ = self
                .send(
                    page,
                    "Emulation.setFocusEmulationEnabled",
                    json!({ "enabled": false }),
                )
                .await;
        }
    }

    pub(super) async fn ensure_rendered_screenshot<F, Fut>(
        &self,
        page: &PageChannel,
        base64: String,
        is_region_crop: bool,
        retry_capture: F,
    ) -> Result<String, String>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<String, String>> + Send,
    {
        let probe_encoded = base64.clone();
        let pixels = tokio::task::spawn_blocking(move || inspect_png_base64(&probe_encoded))
            .await
            .map_err(|error| {
                format!("surface not rendering: screenshot inspection failed ({error})")
            })??;
        if pixels == CapturePixels::Rendered {
            return Ok(base64);
        }
        let state = self.probe_render_state(page).await;
        if classify_capture(pixels, state.as_ref(), is_region_crop).is_ok() {
            return Ok(base64);
        }
        let focus_enabled = self.wake_surface_for_capture(page).await;
        let retry_result = retry_capture().await;
        self.restore_focus_emulation(page, focus_enabled).await;
        let retried = retry_result?;
        let retry_encoded = retried.clone();
        let retried_pixels =
            tokio::task::spawn_blocking(move || inspect_png_base64(&retry_encoded))
                .await
                .map_err(|error| {
                    format!("surface not rendering: screenshot inspection failed ({error})")
                })??;
        if retried_pixels == CapturePixels::Rendered {
            return Ok(retried);
        }
        let after_state = self.probe_render_state(page).await;
        classify_capture(retried_pixels, after_state.as_ref(), is_region_crop)?;
        Ok(retried)
    }

    pub(super) async fn verified_screenshot_base64(
        &self,
        page: &PageChannel,
        full: bool,
        include_overlay: bool,
    ) -> Result<String, String> {
        let first = self.screenshot_base64(page, full, include_overlay).await?;
        self.ensure_rendered_screenshot(page, first, false, || async {
            self.screenshot_base64(page, full, include_overlay).await
        })
        .await
    }
}

#[cfg(test)]
mod tests;
