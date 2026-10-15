use std::path::Path;
use std::sync::Arc;

use image::{Rgba, RgbaImage};
use serde_json::json;

use super::*;
use crate::surface::runtime::{SurfaceEffect, SurfaceResume};
use codeterm_browser::BrowserHostConfig;

const PNG_420X180: &str = "iVBORw0KGgoAAAANSUhEUgAAAaQAAAC0CAYAAAA=";

fn area(x: f64, y: f64, w: f64, h: f64) -> SnipArgs {
    SnipArgs { x, y, w, h }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn facts() -> SnipFacts {
    SnipFacts {
        url: "https://shop.test/cart?x=1".into(),
        title: "Cart\nreview".into(),
        viewport: (1280.0, 720.0),
        scroll: (0.0, 640.4),
    }
}

fn runtime(data_dir: &Path) -> BrowserRuntime {
    BrowserRuntime::new(
        BrowserHostConfig::under_data_dir(data_dir),
        "Jev",
        "owner",
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume::default(),
    )
}

fn ctx(data_dir: &Path) -> SurfaceCallCtx {
    SurfaceCallCtx {
        host: Arc::new(crate::test_support::RecordingSurfaceHost::default()),
        owner_tab_id: None,
        data_dir: data_dir.to_path_buf(),
        actor: None,
    }
}

#[test]
fn browser_snip_rect_rounds_and_rejects_bad_areas() {
    assert_eq!(
        snip_rect(&area(312.4, 96.6, 419.6, 180.2)).unwrap(),
        rect(312.0, 97.0, 420.0, 180.0)
    );
    for bad in [
        area(f64::NAN, 0.0, 10.0, 10.0),
        area(0.0, 0.0, f64::INFINITY, 10.0),
        area(-5.0, 0.0, 10.0, 10.0),
        area(0.0, 0.0, 0.2, 10.0),
        area(0.0, 0.0, 10.0, 9000.0),
    ] {
        assert!(snip_rect(&bad).is_err(), "{bad:?}");
    }
}

const RED: Rgba<u8> = Rgba([220, 30, 30, 255]);
const BLUE: Rgba<u8> = Rgba([20, 40, 200, 255]);

/// A viewport shot whose only red pixels are the box `(x, y, w, h)`.
fn shot_png(size: (u32, u32), red: (u32, u32, u32, u32)) -> String {
    let (rx, ry, rw, rh) = red;
    let image = RgbaImage::from_fn(size.0, size.1, |x, y| {
        if (rx..rx + rw).contains(&x) && (ry..ry + rh).contains(&y) {
            RED
        } else {
            BLUE
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
}

fn decode(base64: &str) -> DynamicImage {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64)
        .unwrap();
    image::load_from_memory(&bytes).unwrap()
}

#[test]
fn browser_snip_capture_shoots_the_viewport_without_clip_or_scale() {
    let params = viewport_shot_params();
    assert_eq!(params, json!({ "format": "png" }));
    assert!(
        params.get("clip").is_none(),
        "a clip re-renders the cast page"
    );
    assert!(params.get("captureBeyondViewport").is_none());
}

#[test]
fn browser_snip_crop_inverts_the_viewer_mapping_into_shot_pixels() {
    let layout = ShotGeometry::from_layout((1280.0, 720.0), (0.0, 640.0));
    assert_eq!(
        crop_box(&rect(312.0, 736.0, 420.0, 180.0), &layout, (2560, 1440)),
        Some(CropBox {
            x: 624,
            y: 192,
            width: 840,
            height: 360
        }),
        "device pixel ratio 2 doubles the box"
    );
    let zoomed = ShotGeometry::from_frame(&FrameMetadata {
        offset_top: 10.0,
        page_scale_factor: 2.0,
        device_width: 500.0,
        device_height: 400.0,
        scroll_offset_x: 0.0,
        scroll_offset_y: 10.0,
        timestamp: None,
    })
    .unwrap();
    assert_eq!(
        crop_box(&rect(10.0, 20.0, 30.0, 40.0), &zoomed, (1000, 800)),
        Some(CropBox {
            x: 40,
            y: 60,
            width: 120,
            height: 160
        })
    );
    assert_eq!(
        crop_box(&rect(1200.0, 700.0, 400.0, 100.0), &layout, (1280, 720)),
        Some(CropBox {
            x: 1200,
            y: 60,
            width: 80,
            height: 100
        }),
        "an area past the edge is clamped to the shot"
    );
    assert_eq!(
        crop_box(&rect(0.0, 0.0, 100.0, 100.0), &layout, (1280, 720)),
        None,
        "an area scrolled out of view leaves nothing"
    );
    let unknown = FrameMetadata {
        device_width: 0.0,
        ..cast_metadata()
    };
    assert_eq!(ShotGeometry::from_frame(&unknown), None);
}

fn cast_metadata() -> FrameMetadata {
    FrameMetadata {
        offset_top: 0.0,
        page_scale_factor: 1.0,
        device_width: 640.0,
        device_height: 360.0,
        scroll_offset_x: 0.0,
        scroll_offset_y: 640.0,
        timestamp: None,
    }
}

#[test]
fn browser_snip_cut_keeps_full_resolution_and_a_chip_sized_thumbnail() {
    let geometry = ShotGeometry::from_frame(&cast_metadata()).unwrap();
    let shot = shot_png((1280, 720), (200, 120, 400, 200));
    let cut = cut_snip(&shot, &rect(100.0, 700.0, 200.0, 100.0), &geometry).unwrap();
    assert_eq!((cut.width, cut.height), (400, 200));
    let crop = decode(&cut.png).to_rgba8();
    assert_eq!(crop.dimensions(), (400, 200));
    assert!(
        crop.pixels().all(|pixel| *pixel == RED),
        "exactly the snipped box, nothing around it"
    );
    let thumbnail = cut.thumbnail.unwrap();
    let data = thumbnail.strip_prefix("data:image/jpeg;base64,").unwrap();
    let small = decode(data);
    assert_eq!(
        (small.width(), small.height()),
        (128, 64),
        "64 CSS px at ratio 2"
    );
    assert!(cut_snip("not png", &rect(0.0, 0.0, 1.0, 1.0), &geometry).is_err());
    assert!(cut_snip(&shot, &rect(0.0, 0.0, 10.0, 10.0), &geometry).is_err());
}

#[test]
fn browser_snip_reads_layout_facts() {
    let metrics = json!({
        "cssLayoutViewport": { "clientWidth": 1, "clientHeight": 1, "pageX": 0, "pageY": 0 },
        "cssVisualViewport": { "clientWidth": 1280.0, "clientHeight": 720.0, "pageX": 12.0, "pageY": 640.0 },
    });
    assert_eq!(layout_facts(&metrics), ((1280.0, 720.0), (12.0, 640.0)));
    assert_eq!(layout_facts(&json!({})), ((0.0, 0.0), (0.0, 0.0)));
}

#[test]
fn browser_snip_markdown_carries_one_image_then_context_then_the_question() {
    let snip = Snip {
        id: "s1".into(),
        rect: rect(312.0, 96.0, 420.0, 180.0),
        png: PNG_420X180.into(),
        facts: facts(),
    };
    let markdown = snip_markdown(&snip, "  why is this cut off?  ", Some("C:\\pad\\a\"b.png"));
    assert_eq!(
        markdown,
        [
            "## Browser snip from the user",
            "<domios type=\"attachment\" kind=\"image\" path=\"C:\\pad\\a&quot;b.png\" />",
            "**Page:** Cart review — `https://shop.test/cart?x=1`",
            "**Viewport:** 1280×720 · **Scroll:** (0, 640)",
            "**Area:** 420×180 at (312, 96) in page CSS px",
            "**User says:** why is this cut off?",
        ]
        .join("\n")
    );
    let text_only = snip_markdown(&snip, "", None);
    assert!(!text_only.contains("type=\"attachment\""));
    assert!(text_only.contains("**Area:** 420×180 at (312, 96)"));
    assert!(text_only.ends_with("**User says:** (no comment)"));
    let long = snip_markdown(&snip, &"x".repeat(SNIP_COMMENT_MAX + 50), None);
    assert!(long.ends_with(&"x".repeat(SNIP_COMMENT_MAX)));
    assert!(!long.contains(&"x".repeat(SNIP_COMMENT_MAX + 1)));
}

#[test]
fn browser_snip_store_is_bounded_and_takes_once() {
    let mut store = SnipStore::default();
    let ids: Vec<String> = (0..6)
        .map(|_| store.keep(rect(0.0, 0.0, 1.0, 1.0), String::new(), facts()))
        .collect();
    assert_eq!(store.len(), PENDING_SNIPS);
    assert!(
        store.take(&ids[0]).is_none(),
        "the oldest capture was evicted"
    );
    assert_eq!(
        store.take(&ids[5]).map(|snip| snip.id),
        Some(ids[5].clone())
    );
    assert!(store.take(&ids[5]).is_none());
}

#[tokio::test]
async fn browser_snip_send_saves_the_crop_into_the_pad_and_delivers_it() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let ctx = ctx(root.path());
    let id =
        runtime
            .snips
            .lock()
            .keep(rect(312.0, 96.0, 420.0, 180.0), PNG_420X180.into(), facts());
    let outcome = runtime
        .view_call(&ctx, "snip.send", json!({ "id": id, "comment": "fix" }))
        .await
        .unwrap();
    let dir = ctx.evidence_dir();
    let saved: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(saved.len(), 1, "exactly one crop is written");
    let path = saved[0].path();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        base64::engine::general_purpose::STANDARD
            .decode(PNG_420X180)
            .unwrap()
    );
    let [SurfaceEffect::DeliverAnnotation {
        id: delivered,
        markdown,
    }] = outcome.effects.as_slice()
    else {
        panic!("one delivery effect");
    };
    assert_eq!(delivered, &id);
    assert_eq!(markdown.matches("type=\"attachment\"").count(), 1);
    assert!(markdown.contains(&attachment_marker(&path.to_string_lossy())));
    assert!(markdown.ends_with("**User says:** fix"));
    assert!(runtime
        .view_call(&ctx, "snip.send", json!({ "id": id, "comment": "again" }))
        .await
        .is_err());
}

#[tokio::test]
async fn browser_snip_without_the_image_or_cancelled_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let runtime = runtime(root.path());
    let ctx = ctx(root.path());
    let keep = || {
        runtime
            .snips
            .lock()
            .keep(rect(0.0, 0.0, 10.0, 10.0), PNG_420X180.into(), facts())
    };
    let text_only = keep();
    let outcome = runtime
        .view_call(
            &ctx,
            "snip.send",
            json!({ "id": text_only, "comment": "just text", "image": false }),
        )
        .await
        .unwrap();
    let [SurfaceEffect::DeliverAnnotation { markdown, .. }] = outcome.effects.as_slice() else {
        panic!("one delivery effect");
    };
    assert!(!markdown.contains("type=\"attachment\""));
    assert!(markdown.contains("**Area:** 10×10 at (0, 0)"));
    let cancelled = keep();
    runtime
        .view_call(&ctx, "snip.cancel", json!({ "id": cancelled }))
        .await
        .unwrap();
    assert_eq!(runtime.snips.lock().len(), 0);
    assert!(!ctx.evidence_dir().exists(), "nothing reached the pad");
}
