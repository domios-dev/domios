use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::service::browser::BrowserHostConfig;
use crate::service::surface::runtime::{SurfaceEffect, SurfaceResume};

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
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume::default(),
    )
}

fn ctx(data_dir: &Path) -> SurfaceCallCtx {
    SurfaceCallCtx {
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

#[test]
fn browser_snip_capture_clips_the_area_at_full_resolution_inside_the_viewport() {
    let area = rect(312.0, 736.0, 420.0, 180.0);
    assert_eq!(
        capture_params(&area),
        json!({
            "format": "png",
            "clip": { "x": 312.0, "y": 736.0, "width": 420.0, "height": 180.0, "scale": 1.0 },
            "captureBeyondViewport": false,
        })
    );
    let thumb = thumbnail_params(&area);
    assert_eq!(thumb["format"], "jpeg");
    assert_eq!(thumb["captureBeyondViewport"], false);
    assert_eq!(thumb["clip"]["x"], 312.0);
    assert_eq!(thumb["clip"]["width"], 420.0);
    let scale = thumb["clip"]["scale"].as_f64().unwrap();
    assert!(
        (scale * 420.0 - 64.0).abs() < 1e-9,
        "longest side shrinks to the thumb"
    );
    assert_eq!(
        thumbnail_params(&rect(0.0, 0.0, 20.0, 10.0))["clip"]["scale"],
        1.0
    );
}

#[test]
fn browser_snip_reads_png_size_and_layout_facts() {
    assert_eq!(png_size(PNG_420X180), Some((420, 180)));
    assert_eq!(png_size("aGVsbG8="), None);
    assert_eq!(png_size(""), None);
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
