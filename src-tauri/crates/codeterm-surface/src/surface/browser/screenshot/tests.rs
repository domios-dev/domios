use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use codeterm_browser::{
    cdp::{CdpClient, CdpConnection, CdpError, EventStream},
    cdp_wire::{CdpEvent, EventFilter},
};
use image::{DynamicImage, Rgba, RgbaImage};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use super::*;

fn solid_png_base64(width: u32, height: u32, pixel: [u8; 4]) -> String {
    let image = RgbaImage::from_pixel(width, height, Rgba(pixel));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
}

fn rendered_png_base64(width: u32, height: u32) -> String {
    let mut image = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    image.put_pixel(width / 2, height / 2, Rgba([29, 155, 240, 255]));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
}

fn visible_content_state() -> PageRenderState {
    PageRenderState {
        visibility_state: "visible".into(),
        hidden: false,
        has_focus: true,
        has_text: true,
        visible_elements: 3,
        viewport_width: 1585.0,
        viewport_height: 900.0,
    }
}

#[test]
fn screenshot_pixel_inspector_distinguishes_uniform_black_from_rendered_frames() {
    let black = solid_png_base64(32, 18, [0, 0, 0, 255]);
    assert_eq!(
        inspect_png_base64(&black).unwrap(),
        CapturePixels::Uniform {
            rgba: [0, 0, 0, 255],
            width: 32,
            height: 18,
        }
    );

    let white = solid_png_base64(16, 16, [255, 255, 255, 255]);
    assert_eq!(
        inspect_png_base64(&white).unwrap(),
        CapturePixels::Uniform {
            rgba: [255, 255, 255, 255],
            width: 16,
            height: 16,
        }
    );

    let rendered = rendered_png_base64(32, 18);
    assert_eq!(
        inspect_png_base64(&rendered).unwrap(),
        CapturePixels::Rendered
    );

    assert!(inspect_png_base64("not-valid-base64!!!").is_err());
    assert!(inspect_png_base64("aGVsbG8=").is_err());
}

#[test]
fn screenshot_classifier_rejects_blank_frames_on_rendered_or_hidden_pages_and_allows_empty_pages() {
    let black_uniform = CapturePixels::Uniform {
        rgba: [0, 0, 0, 255],
        width: 1585,
        height: 900,
    };
    let white_uniform = CapturePixels::Uniform {
        rgba: [255, 255, 255, 255],
        width: 1585,
        height: 900,
    };
    let content = visible_content_state();

    assert_eq!(
        classify_capture(CapturePixels::Rendered, Some(&content), false),
        Ok(())
    );
    let black_err = classify_capture(black_uniform, Some(&content), false).unwrap_err();
    assert!(
        black_err.starts_with("surface not rendering:") && black_err.contains("all-black 1585x900"),
        "{black_err}"
    );
    let white_err = classify_capture(white_uniform, Some(&content), false).unwrap_err();
    assert!(
        white_err.starts_with("surface not rendering:") && white_err.contains("uniform blank"),
        "{white_err}"
    );

    let mut hidden_state = content.clone();
    hidden_state.visibility_state = "hidden".into();
    hidden_state.hidden = true;
    let hidden_err = classify_capture(black_uniform, Some(&hidden_state), false).unwrap_err();
    assert!(
        hidden_err.starts_with("surface not rendering:") && hidden_err.contains("hidden"),
        "{hidden_err}"
    );

    // Intentionally empty pages (such as about:blank) and non-black region crops are valid.
    let blank_page = PageRenderState {
        visibility_state: "visible".into(),
        hidden: false,
        has_focus: true,
        has_text: false,
        visible_elements: 0,
        viewport_width: 1600.0,
        viewport_height: 900.0,
    };
    assert_eq!(
        classify_capture(white_uniform, Some(&blank_page), false),
        Ok(())
    );
    assert_eq!(
        classify_capture(black_uniform, Some(&blank_page), false),
        Ok(())
    );
    assert_eq!(
        classify_capture(white_uniform, Some(&content), true),
        Ok(())
    );

    // Missing render state fails closed on full-viewport all-black captures.
    assert!(classify_capture(black_uniform, None, false)
        .unwrap_err()
        .starts_with("surface not rendering:"));
}

struct ScreenshotFakeConnection {
    closed: watch::Sender<Option<String>>,
    subscribers: Mutex<Vec<mpsc::Sender<Arc<CdpEvent>>>>,
    shots: Vec<String>,
    shot_index: AtomicUsize,
    render_state: Value,
    calls: Mutex<Vec<(String, Value)>>,
}

#[async_trait::async_trait]
impl CdpConnection for ScreenshotFakeConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/screenshot-test"
    }
    fn closed(&self) -> watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, reason: &str) {
        self.closed.send_replace(Some(reason.into()));
    }
    fn subscribe(&self, _filter: EventFilter) -> EventStream {
        let (tx, rx) = mpsc::channel(8);
        self.subscribers.lock().push(tx);
        EventStream::from_receiver(rx)
    }
    async fn send_to(
        &self,
        _: Option<&str>,
        method: &str,
        params: Value,
        _: Duration,
    ) -> Result<Value, CdpError> {
        self.calls.lock().push((method.into(), params.clone()));
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId": "session"}),
            "Page.getLayoutMetrics" => json!({
                "cssLayoutViewport": {"pageX": 0, "pageY": 0, "clientWidth": 32, "clientHeight": 18}
            }),
            "Page.captureScreenshot" => {
                let idx = self.shot_index.fetch_add(1, Ordering::SeqCst);
                let data = self
                    .shots
                    .get(idx)
                    .or_else(|| self.shots.last())
                    .cloned()
                    .unwrap_or_default();
                json!({"data": data})
            }
            "Runtime.evaluate" => {
                let expr = params["expression"].as_str().unwrap_or("");
                if expr.contains("visibilityState") {
                    json!({"result": {"value": self.render_state.clone()}})
                } else {
                    json!({"result": {"value": true}})
                }
            }
            _ => json!({}),
        })
    }
}

async fn screenshot_fixture(
    shots: Vec<String>,
    render_state: Value,
) -> (
    BrowserRuntime,
    PageChannel,
    Arc<ScreenshotFakeConnection>,
    tempfile::TempDir,
) {
    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    let (closed, _) = watch::channel(None);
    let connection = Arc::new(ScreenshotFakeConnection {
        closed,
        subscribers: Mutex::new(Vec::new()),
        shots,
        shot_index: AtomicUsize::new(0),
        render_state,
        calls: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("main")
        .await
        .unwrap();
    runtime.shared.bind(&page);
    (runtime, page, connection, root)
}

#[tokio::test]
async fn verified_screenshot_wakes_surface_and_recovers_after_initial_black_capture() {
    let black = solid_png_base64(32, 18, [0, 0, 0, 255]);
    let rendered = rendered_png_base64(32, 18);
    let (runtime, page, connection, _root) = screenshot_fixture(
        vec![black, rendered.clone()],
        json!({
            "visibilityState": "visible",
            "hidden": false,
            "hasFocus": false,
            "hasText": true,
            "visibleElements": 3,
            "viewportWidth": 32,
            "viewportHeight": 18
        }),
    )
    .await;

    let result = runtime
        .verified_screenshot_base64(&page, false, false)
        .await
        .unwrap();
    assert_eq!(result, rendered);

    let calls = connection.calls.lock();
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _)| method == "Page.captureScreenshot")
            .count(),
        2
    );
    assert!(calls
        .iter()
        .any(|(method, _)| method == "Page.bringToFront"));
    let focus_calls: Vec<_> = calls
        .iter()
        .filter(|(method, _)| method == "Emulation.setFocusEmulationEnabled")
        .map(|(_, params)| params["enabled"].as_bool())
        .collect();
    assert_eq!(focus_calls, vec![Some(true), Some(false)]);
}

#[tokio::test]
async fn verified_screenshot_fails_honestly_when_capture_stays_blank_black() {
    let black = solid_png_base64(32, 18, [0, 0, 0, 255]);
    let (runtime, page, connection, _root) = screenshot_fixture(
        vec![black.clone(), black],
        json!({
            "visibilityState": "visible",
            "hidden": false,
            "hasFocus": true,
            "hasText": true,
            "visibleElements": 3,
            "viewportWidth": 32,
            "viewportHeight": 18
        }),
    )
    .await;

    let err = runtime
        .verified_screenshot_base64(&page, false, false)
        .await
        .unwrap_err();
    assert!(
        err.starts_with("surface not rendering:")
            && err.contains("all-black 32x18")
            && err.contains("close and reopen the browser surface"),
        "{err}"
    );

    let calls = connection.calls.lock();
    let focus_calls: Vec<_> = calls
        .iter()
        .filter(|(method, _)| method == "Emulation.setFocusEmulationEnabled")
        .map(|(_, params)| params["enabled"].as_bool())
        .collect();
    assert_eq!(focus_calls, vec![Some(true), Some(false)]);
}

#[tokio::test]
async fn verified_screenshot_skips_recovery_when_first_capture_has_rendered_pixels() {
    let rendered = rendered_png_base64(32, 18);
    let (runtime, page, connection, _root) = screenshot_fixture(
        vec![rendered.clone()],
        json!({
            "visibilityState": "visible",
            "hidden": false,
            "hasFocus": true,
            "hasText": true,
            "visibleElements": 3,
            "viewportWidth": 32,
            "viewportHeight": 18
        }),
    )
    .await;

    let result = runtime
        .verified_screenshot_base64(&page, false, false)
        .await
        .unwrap();
    assert_eq!(result, rendered);

    let calls = connection.calls.lock();
    assert_eq!(
        calls
            .iter()
            .filter(|(method, _)| method == "Page.captureScreenshot")
            .count(),
        1
    );
    assert!(!calls
        .iter()
        .any(|(method, _)| method == "Emulation.setFocusEmulationEnabled"));
}
