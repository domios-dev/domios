use super::*;
use codeterm_browser::{
    cdp::{CdpClient, CdpConnection, CdpError, EventStream},
    cdp_wire::{CdpEvent, EventFilter},
};
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

fn rect(left: f64, top: f64, width: f64, height: f64) -> Rect {
    Rect {
        left,
        top,
        width,
        height,
    }
}

fn node(id: usize, parent: Option<usize>, tag: &str) -> DomNode {
    DomNode {
        id,
        parent,
        tag: tag.into(),
        position: "static".into(),
        ..Default::default()
    }
}

fn base_page_nodes() -> Vec<DomNode> {
    let mut html = node(0, None, "html");
    html.landmark_or_chrome = true;
    html.rect = Some(rect(0.0, 0.0, 1280.0, 800.0));
    let mut body = node(1, Some(0), "body");
    body.landmark_or_chrome = true;
    body.rect = Some(rect(0.0, 0.0, 1280.0, 800.0));
    let mut main = node(2, Some(1), "main");
    main.role = "main".into();
    main.landmark_or_chrome = true;
    main.rect = Some(rect(0.0, 0.0, 1000.0, 800.0));
    vec![html, body, main]
}

fn notification_card_scene(overlay_transparent: bool, overlay_rect: Rect) -> HitScene {
    let mut nodes = base_page_nodes();
    let mut card = node(3, Some(2), "article");
    card.role = "article".into();
    card.clickable_container = true;
    card.cursor_pointer = true;
    card.position = "relative".into();
    card.rect = Some(rect(20.0, 100.0, 500.0, 80.0));

    let mut link = node(4, Some(3), "a");
    link.role = "link".into();
    link.name = "Ishwar Dhote commented on your post.".into();
    link.href = "https://www.linkedin.com/feed/update/1".into();
    link.interactive = true;
    link.rect = Some(rect(80.0, 112.0, 340.0, 44.0));

    let mut overlay = node(5, Some(3), "div");
    overlay.position = "absolute".into();
    overlay.transparent = overlay_transparent;
    overlay.cursor_pointer = true;
    overlay.rect = Some(overlay_rect);

    nodes.extend([card, link, overlay]);
    HitScene {
        target: 4,
        stack: vec![5, 4, 3, 2, 1, 0],
        nodes,
    }
}

#[test]
fn browser_pointer_hit_test_fails_closed() {
    assert_eq!(
        hit_test_result(&json!({"result":{"value":"clear"}})),
        Ok(())
    );
    for reason in [
        "target_occluded: covered by button#cover",
        "stale: detached pointer target",
    ] {
        assert_eq!(
            hit_test_result(&json!({"result":{"value":reason}})),
            Err(reason.into())
        );
    }
    for reply in [
        json!({}),
        json!({"result":{"value":true}}),
        json!({"exceptionDetails":{}}),
    ] {
        assert_eq!(
            hit_test_result(&reply),
            Err("target_occluded: pointer hit-test unavailable".into())
        );
    }
}

#[test]
fn browser_hit_test_allows_target_descendant_and_ancestor_but_refuses_document_root() {
    let mut nodes = base_page_nodes();
    let mut card = node(3, Some(2), "article");
    card.rect = Some(rect(0.0, 0.0, 500.0, 80.0));
    let mut link = node(4, Some(3), "a");
    link.role = "link".into();
    link.href = "https://example.com/post".into();
    link.interactive = true;
    let span = node(5, Some(4), "span");
    nodes.extend([card, link, span]);

    let direct = HitScene {
        target: 4,
        stack: vec![4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(classify_scene(&direct), Ok(()));

    let descendant = HitScene {
        target: 4,
        stack: vec![5, 4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(classify_scene(&descendant), Ok(()));

    let ancestor = HitScene {
        target: 4,
        stack: vec![3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(classify_scene(&ancestor), Ok(()));

    let body_hit = HitScene {
        target: 4,
        stack: vec![1, 0],
        nodes,
    };
    assert_eq!(
        classify_scene(&body_hit),
        Err("target_occluded: covered by body".into())
    );
}

#[test]
fn browser_hit_test_allows_full_card_transparent_overlay_and_same_destination_anchor() {
    let transparent_card = notification_card_scene(true, rect(20.0, 100.0, 500.0, 80.0));
    assert_eq!(classify_scene(&transparent_card), Ok(()));

    // Card item without explicit clickable_container still routes when overlay has cursor: pointer.
    let mut article_cursor_overlay = transparent_card.clone();
    article_cursor_overlay.nodes[3].clickable_container = false;
    article_cursor_overlay.nodes[3].cursor_pointer = false;
    assert_eq!(classify_scene(&article_cursor_overlay), Ok(()));

    // Opaque sibling layer inside the card is refused.
    let opaque_card = notification_card_scene(false, rect(20.0, 100.0, 500.0, 80.0));
    assert_eq!(
        classify_scene(&opaque_card),
        Err("target_occluded: covered by div".into())
    );

    // Transparent overlay escaping its container bounds is refused.
    let oversized_overlay = notification_card_scene(true, rect(0.0, 0.0, 1280.0, 800.0));
    assert_eq!(
        classify_scene(&oversized_overlay),
        Err("target_occluded: covered by div".into())
    );

    // Sibling stretched anchor with the exact same destination is allowed.
    let mut same_href = transparent_card.clone();
    same_href.nodes[5].tag = "a".into();
    same_href.nodes[5].role = "link".into();
    same_href.nodes[5].interactive = true;
    same_href.nodes[5].href = "https://www.linkedin.com/feed/update/1".into();
    assert_eq!(classify_scene(&same_href), Ok(()));

    // Sibling anchor pointing elsewhere is refused and named.
    let mut other_href = same_href;
    other_href.nodes[5].href = "https://www.linkedin.com/in/ishwar".into();
    other_href.nodes[5].name = "View profile".into();
    assert_eq!(
        classify_scene(&other_href),
        Err("target_occluded: covered by link \"View profile\"".into())
    );
}

#[test]
fn browser_hit_test_refuses_real_modals_banners_and_other_controls_with_role_and_name() {
    // Real modal dialog covering the card.
    let mut nodes = base_page_nodes();
    let mut card = node(3, Some(2), "article");
    card.clickable_container = true;
    card.rect = Some(rect(20.0, 100.0, 500.0, 80.0));
    let mut link = node(4, Some(3), "a");
    link.role = "link".into();
    link.name = "Ishwar Dhote commented on your post.".into();
    link.href = "https://www.linkedin.com/feed/update/1".into();
    link.interactive = true;

    let mut dialog = node(5, Some(1), "div");
    dialog.role = "dialog".into();
    dialog.name = "Cookie preferences".into();
    dialog.modal = true;
    dialog.landmark_or_chrome = true;
    dialog.position = "fixed".into();
    dialog.rect = Some(rect(100.0, 80.0, 600.0, 400.0));
    let dialog_copy = node(6, Some(5), "p");

    nodes.extend([card.clone(), link.clone(), dialog, dialog_copy]);
    let modal_scene = HitScene {
        target: 4,
        stack: vec![6, 5, 4, 3, 2, 1, 0],
        nodes,
    };
    assert_eq!(
        classify_scene(&modal_scene),
        Err("target_occluded: covered by dialog \"Cookie preferences\"".into())
    );

    // Transparent fixed modal backdrop wrapping a dialog still refuses and names the dialog.
    let mut backdrop_nodes = base_page_nodes();
    backdrop_nodes.extend([card.clone(), link.clone()]);
    let mut backdrop = node(5, Some(1), "div");
    backdrop.position = "fixed".into();
    backdrop.transparent = true;
    backdrop.contained_modal = Some(ContainedModal {
        role: "dialog".into(),
        tag: "div".into(),
        dom_id: String::new(),
        name: "Sign in".into(),
    });
    backdrop_nodes.push(backdrop);
    let backdrop_scene = HitScene {
        target: 4,
        stack: vec![5, 4, 3, 2, 1, 0],
        nodes: backdrop_nodes,
    };
    assert_eq!(
        classify_scene(&backdrop_scene),
        Err("target_occluded: covered by dialog \"Sign in\"".into())
    );

    // Cookie banner refuses and reports its role and name.
    let mut banner_nodes = base_page_nodes();
    banner_nodes.extend([card.clone(), link.clone()]);
    let mut banner = node(5, Some(1), "div");
    banner.role = "banner".into();
    banner.name = "Cookie consent".into();
    banner.landmark_or_chrome = true;
    banner.position = "fixed".into();
    let banner_text = node(6, Some(5), "p");
    banner_nodes.extend([banner, banner_text]);
    let banner_scene = HitScene {
        target: 4,
        stack: vec![6, 5, 4, 3, 2, 1, 0],
        nodes: banner_nodes,
    };
    assert_eq!(
        classify_scene(&banner_scene),
        Err("target_occluded: covered by banner \"Cookie consent\"".into())
    );

    // Different interactive control inside the same card refuses, even when hitting a transparent child span.
    let mut control_nodes = base_page_nodes();
    control_nodes.extend([card, link]);
    let mut more_btn = node(5, Some(3), "button");
    more_btn.role = "button".into();
    more_btn.name = "More options".into();
    more_btn.interactive = true;
    more_btn.transparent = true;
    more_btn.rect = Some(rect(460.0, 110.0, 40.0, 40.0));
    let mut icon_span = node(6, Some(5), "span");
    icon_span.transparent = true;
    icon_span.rect = Some(rect(460.0, 110.0, 40.0, 40.0));
    control_nodes.extend([more_btn, icon_span]);
    let control_scene = HitScene {
        target: 4,
        stack: vec![6, 5, 4, 3, 2, 1, 0],
        nodes: control_nodes,
    };
    assert_eq!(
        classify_scene(&control_scene),
        Err("target_occluded: covered by button \"More options\"".into())
    );

    // Real modal sandwiched underneath a transparent card overlay still refuses.
    let mut sandwich = notification_card_scene(true, rect(20.0, 100.0, 500.0, 80.0));
    let mut inner_modal = node(6, Some(1), "div");
    inner_modal.role = "dialog".into();
    inner_modal.name = "Confirm".into();
    inner_modal.modal = true;
    inner_modal.landmark_or_chrome = true;
    sandwich.nodes.push(inner_modal);
    sandwich.stack = vec![5, 6, 4, 3, 2, 1, 0];
    assert_eq!(
        classify_scene(&sandwich),
        Err("target_occluded: covered by dialog \"Confirm\"".into())
    );
}

#[test]
fn browser_hit_test_allows_associated_label_over_checkbox_and_radio_but_refuses_unrelated_label() {
    // Sibling <label for="comments-cb"> (and its non-interactive child <span>) over a checkbox.
    let mut nodes = base_page_nodes();
    let mut input = node(3, Some(2), "input");
    input.dom_id = "comments-cb".into();
    input.role = "checkbox".into();
    input.name = "Comments".into();
    input.interactive = true;

    let mut assoc_label = node(4, Some(2), "label");
    assoc_label.name = "Comments".into();
    assoc_label.associated_label = true;

    let label_span = node(5, Some(4), "span");

    let mut nested_link = node(6, Some(4), "a");
    nested_link.role = "link".into();
    nested_link.name = "Terms".into();
    nested_link.href = "https://example.com/terms".into();
    nested_link.interactive = true;

    let mut other_label = node(7, Some(2), "label");
    other_label.name = "Reactions".into();

    nodes.extend([input, assoc_label, label_span, nested_link, other_label]);

    let label_direct = HitScene {
        target: 3,
        stack: vec![4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(classify_scene(&label_direct), Ok(()));

    let label_descendant = HitScene {
        target: 3,
        stack: vec![5, 4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(classify_scene(&label_descendant), Ok(()));

    // Interactive link inside the associated label intercepts the click, so it must refuse.
    let label_nested_link = HitScene {
        target: 3,
        stack: vec![6, 4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(
        classify_scene(&label_nested_link),
        Err("target_occluded: covered by link \"Terms\"".into())
    );

    // Unrelated <label> for a different control must refuse.
    let unrelated_label = HitScene {
        target: 3,
        stack: vec![7, 3, 2, 1, 0],
        nodes,
    };
    assert_eq!(
        classify_scene(&unrelated_label),
        Err("target_occluded: covered by label \"Reactions\"".into())
    );

    // Wrapping <label><input type="radio"><span class="dot"></span></label> allows hitting the dot.
    let mut radio_nodes = base_page_nodes();
    let mut wrap_label = node(3, Some(2), "label");
    wrap_label.name = "Recent".into();
    wrap_label.associated_label = true;
    let mut radio = node(4, Some(3), "input");
    radio.role = "radio".into();
    radio.name = "Recent".into();
    radio.interactive = true;
    let custom_dot = node(5, Some(3), "span");
    radio_nodes.extend([wrap_label, radio, custom_dot]);

    let wrapped_radio = HitScene {
        target: 4,
        stack: vec![5, 3, 2, 1, 0],
        nodes: radio_nodes,
    };
    assert_eq!(classify_scene(&wrapped_radio), Ok(()));
}

#[test]
fn browser_hit_test_candidate_points_stay_in_bounds_across_shapes() {
    let wide = json!({"quads": [[0, 0, 200, 0, 200, 40, 0, 40]]});
    let points = candidate_points(&wide);
    assert_eq!(points.first(), Some(&(100.0, 20.0)));
    assert_eq!(points.len(), 9);
    for &(x, y) in &points {
        assert!(x > 0.0 && x < 200.0, "x out of bounds: {x}");
        assert!(y > 0.0 && y < 40.0, "y out of bounds: {y}");
    }
    assert!(points
        .iter()
        .any(|&(x, y)| (x - 4.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9));
    assert!(points
        .iter()
        .any(|&(x, y)| (x - 196.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9));
    assert!(points
        .iter()
        .any(|&(x, y)| (x - 4.0).abs() < 1e-9 && (y - 4.0).abs() < 1e-9));
    assert!(points
        .iter()
        .any(|&(x, y)| (x - 196.0).abs() < 1e-9 && (y - 36.0).abs() < 1e-9));

    let tiny = json!({"quads": [[10, 10, 14, 10, 14, 14, 10, 14]]});
    let tiny_points = candidate_points(&tiny);
    assert_eq!(tiny_points.first(), Some(&(12.0, 12.0)));
    for &(x, y) in &tiny_points {
        assert!(x > 10.0 && x < 14.0, "tiny x out of bounds: {x}");
        assert!(y > 10.0 && y < 14.0, "tiny y out of bounds: {y}");
    }

    // Rotated diamond centered at (50, 50) with radius 20: |x - 50| + |y - 50| < 20.
    let diamond = json!({"quads": [[50, 30, 70, 50, 50, 70, 30, 50]]});
    let diamond_points = candidate_points(&diamond);
    assert_eq!(diamond_points.first(), Some(&(50.0, 50.0)));
    for &(x, y) in &diamond_points {
        assert!(
            (x - 50.0).abs() + (y - 50.0).abs() < 20.0,
            "rotated point ({x}, {y}) outside diamond"
        );
    }

    let wrapped = json!({
        "quads": [
            [0, 0, 200, 0, 200, 20, 0, 20],
            [0, 22, 120, 22, 120, 42, 0, 42]
        ]
    });
    let wrapped_points = candidate_points(&wrapped);
    assert!(wrapped_points.contains(&(60.0, 32.0)));
    assert!(wrapped_points.len() <= MAX_PROBE_POINTS);

    assert!(candidate_points(&json!({"quads": []})).is_empty());
    assert!(candidate_points(&json!({"quads": [[1, 2]]})).is_empty());
}

struct PartialOcclusionConnection {
    closed: watch::Sender<Option<String>>,
    subscribers: Mutex<Vec<mpsc::Sender<Arc<CdpEvent>>>>,
    occlude_all: bool,
    probed_points: Mutex<Vec<(f64, f64)>>,
    input_commands: Mutex<Vec<(String, Value)>>,
    eval_calls: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl CdpConnection for PartialOcclusionConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/hit-test"
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
        if method.starts_with("Input.") {
            self.input_commands
                .lock()
                .push((method.into(), params.clone()));
        }
        Ok(match method {
            "Target.attachToTarget" => json!({"sessionId": "session"}),
            "Page.getFrameTree" => json!({"frameTree": {"frame": {"id": "main", "loaderId": "1"}}}),
            "DOM.getContentQuads" => json!({"quads": [[0, 0, 200, 0, 200, 40, 0, 40]]}),
            "DOM.resolveNode" => json!({"object": {"objectId": "link-obj"}}),
            "Runtime.callFunctionOn"
                if params["functionDeclaration"]
                    .as_str()
                    .is_some_and(|src| src.contains("function domiosHitTest")) =>
            {
                let x = params["arguments"][0]["value"].as_f64().unwrap_or(0.0);
                let y = params["arguments"][1]["value"].as_f64().unwrap_or(0.0);
                self.probed_points.lock().push((x, y));
                let scene = if self.occlude_all {
                    json!({
                        "target": 2,
                        "stack": [3, 2, 1, 0],
                        "nodes": [
                            {"id": 0, "parent": null, "tag": "html", "landmarkOrChrome": true},
                            {"id": 1, "parent": 0, "tag": "body", "landmarkOrChrome": true},
                            {"id": 2, "parent": 1, "tag": "a", "role": "link", "name": "Notification", "href": "https://example.com/n/1", "interactive": true},
                            {"id": 3, "parent": 1, "tag": "div", "role": "dialog", "name": "Sign in", "modal": true, "landmarkOrChrome": true, "position": "fixed"}
                        ]
                    })
                } else if x >= 80.0 {
                    json!({
                        "target": 2,
                        "stack": [3, 2, 1, 0],
                        "nodes": [
                            {"id": 0, "parent": null, "tag": "html", "landmarkOrChrome": true},
                            {"id": 1, "parent": 0, "tag": "article", "clickableContainer": true},
                            {"id": 2, "parent": 1, "tag": "a", "role": "link", "name": "Notification", "href": "https://example.com/n/1", "interactive": true},
                            {"id": 3, "parent": 1, "tag": "button", "role": "button", "name": "More options", "interactive": true}
                        ]
                    })
                } else {
                    json!({
                        "target": 2,
                        "stack": [2, 1, 0],
                        "nodes": [
                            {"id": 0, "parent": null, "tag": "html", "landmarkOrChrome": true},
                            {"id": 1, "parent": 0, "tag": "article", "clickableContainer": true},
                            {"id": 2, "parent": 1, "tag": "a", "role": "link", "name": "Notification", "href": "https://example.com/n/1", "interactive": true}
                        ]
                    })
                };
                json!({"result": {"value": scene}})
            }
            "Runtime.callFunctionOn" => {
                if let Some(src) = params["functionDeclaration"].as_str() {
                    self.eval_calls.lock().push(src.to_string());
                }
                json!({"result": {}})
            }
            "Runtime.evaluate" => {
                if let Some(expr) = params["expression"].as_str() {
                    self.eval_calls.lock().push(expr.to_string());
                }
                json!({"result": {}})
            }
            _ => json!({}),
        })
    }
}

async fn hit_test_runtime(
    occlude_all: bool,
) -> (
    BrowserRuntime,
    PageChannel,
    Arc<PartialOcclusionConnection>,
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
    let connection = Arc::new(PartialOcclusionConnection {
        closed,
        subscribers: Mutex::new(Vec::new()),
        occlude_all,
        probed_points: Mutex::new(Vec::new()),
        input_commands: Mutex::new(Vec::new()),
        eval_calls: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("main")
        .await
        .unwrap();
    runtime.shared.bind(&page);
    (runtime, page, connection, root)
}

#[tokio::test(start_paused = true)]
async fn browser_click_probes_alternate_in_bounds_point_and_dispatches_real_input() {
    let (runtime, page, connection, _root) = hit_test_runtime(false).await;
    runtime.click_node(&page, 7).await.unwrap();

    let probed = connection.probed_points.lock().clone();
    assert_eq!(probed.len(), 3);
    assert!((probed[0].0 - 100.0).abs() < 1e-9 && (probed[0].1 - 20.0).abs() < 1e-9);
    assert!((probed[1].0 - 4.0).abs() < 1e-9 && (probed[1].1 - 20.0).abs() < 1e-9);
    assert!((probed[2].0 - 4.0).abs() < 1e-9 && (probed[2].1 - 20.0).abs() < 1e-9);

    let inputs = connection.input_commands.lock().clone();
    assert_eq!(inputs.len(), 3);
    for (method, params) in &inputs {
        assert_eq!(method, "Input.dispatchMouseEvent");
        assert!((params["x"].as_f64().unwrap() - 4.0).abs() < 1e-9);
        assert!((params["y"].as_f64().unwrap() - 20.0).abs() < 1e-9);
    }
    assert!(
        connection
            .eval_calls
            .lock()
            .iter()
            .all(|src| !src.contains(".click(")),
        "must never fall back to synthetic DOM .click()"
    );
}

#[tokio::test(start_paused = true)]
async fn browser_click_refuses_when_all_in_bounds_points_are_occluded_by_modal() {
    let (runtime, page, connection, _root) = hit_test_runtime(true).await;
    let error = runtime.click_node(&page, 7).await.unwrap_err();
    assert_eq!(error, "target_occluded: covered by dialog \"Sign in\"");
    assert_eq!(connection.probed_points.lock().len(), 9);
    assert!(connection.input_commands.lock().is_empty());
}

#[test]
fn browser_hit_test_reply_composer_card_allows_nested_button_and_refuses_covering_wrapper() {
    let mut nodes = base_page_nodes();
    let mut composer = node(3, Some(2), "section");
    composer.dom_id = "composer-card".into();
    composer.position = "relative".into();
    composer.rect = Some(rect(200.0, 200.0, 600.0, 280.0));

    let mut toolbar = node(4, Some(3), "div");
    toolbar.dom_id = "toolbar".into();
    toolbar.rect = Some(rect(216.0, 420.0, 568.0, 44.0));

    let mut reply_btn = node(5, Some(4), "button");
    reply_btn.dom_id = "reply-btn".into();
    reply_btn.role = "button".into();
    reply_btn.name = "Reply".into();
    reply_btn.interactive = true;
    reply_btn.clickable_container = true;
    reply_btn.cursor_pointer = true;
    reply_btn.rect = Some(rect(700.0, 424.0, 84.0, 36.0));

    let mut inner_div = node(6, Some(5), "div");
    inner_div.dom_id = "reply-inner".into();
    let mut inner_span = node(7, Some(6), "span");
    inner_span.dom_id = "reply-label".into();

    // Transparent full-card wrapper div inside a non-clickable composer card (no bypass allowed).
    let mut transparent_wrapper = node(8, Some(3), "div");
    transparent_wrapper.dom_id = "card-overlay".into();
    transparent_wrapper.position = "absolute".into();
    transparent_wrapper.transparent = true;
    transparent_wrapper.rect = Some(rect(200.0, 200.0, 600.0, 280.0));

    // Opaque sticky footer covering the reply button.
    let mut sticky_footer = node(9, Some(1), "div");
    sticky_footer.dom_id = "sticky-footer".into();
    sticky_footer.position = "fixed".into();
    sticky_footer.transparent = false;
    sticky_footer.rect = Some(rect(0.0, 852.0, 1585.0, 48.0));

    nodes.extend([
        composer,
        toolbar,
        reply_btn,
        inner_div,
        inner_span,
        transparent_wrapper,
        sticky_footer,
    ]);

    let clear_nested = HitScene {
        target: 5,
        stack: vec![7, 6, 5, 4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(classify_scene(&clear_nested), Ok(()));

    let covered_by_wrapper = HitScene {
        target: 5,
        stack: vec![8, 7, 6, 5, 4, 3, 2, 1, 0],
        nodes: nodes.clone(),
    };
    assert_eq!(
        classify_scene(&covered_by_wrapper),
        Err("target_occluded: covered by div#card-overlay".into())
    );

    let covered_by_footer = HitScene {
        target: 5,
        stack: vec![9, 7, 6, 5, 4, 3, 2, 1, 0],
        nodes,
    };
    assert_eq!(
        classify_scene(&covered_by_footer),
        Err("target_occluded: covered by div#sticky-footer".into())
    );
}

#[tokio::test(start_paused = true)]
async fn browser_click_reply_composer_probes_clear_edge_above_partial_sticky_footer() {
    struct StickyFooterConnection {
        closed: watch::Sender<Option<String>>,
        subscribers: Mutex<Vec<mpsc::Sender<Arc<CdpEvent>>>>,
        probed_points: Mutex<Vec<(f64, f64)>>,
        input_commands: Mutex<Vec<(String, Value)>>,
    }

    #[async_trait::async_trait]
    impl CdpConnection for StickyFooterConnection {
        fn endpoint(&self) -> &str {
            "ws://127.0.0.1:1/sticky-footer"
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
            if method.starts_with("Input.") {
                self.input_commands
                    .lock()
                    .push((method.into(), params.clone()));
            }
            Ok(match method {
                "Target.attachToTarget" => json!({"sessionId": "session"}),
                "Page.getFrameTree" => {
                    json!({"frameTree": {"frame": {"id": "main", "loaderId": "1"}}})
                }
                "DOM.getContentQuads" => {
                    json!({"quads": [[700, 836, 784, 836, 784, 876, 700, 876]]})
                }
                "DOM.resolveNode" => json!({"object": {"objectId": "reply-obj"}}),
                "Runtime.callFunctionOn"
                    if params["functionDeclaration"]
                        .as_str()
                        .is_some_and(|src| src.contains("function domiosHitTest")) =>
                {
                    let x = params["arguments"][0]["value"].as_f64().unwrap_or(0.0);
                    let y = params["arguments"][1]["value"].as_f64().unwrap_or(0.0);
                    self.probed_points.lock().push((x, y));
                    // Sticky footer covers y >= 852 (including the button's center y = 856).
                    let scene = if y >= 852.0 {
                        json!({
                            "target": 2,
                            "stack": [3, 2, 1, 0],
                            "nodes": [
                                {"id": 0, "parent": null, "tag": "html", "landmarkOrChrome": true},
                                {"id": 1, "parent": 0, "tag": "body", "landmarkOrChrome": true},
                                {"id": 2, "parent": 1, "tag": "button", "domId": "reply-btn", "role": "button", "name": "Reply", "interactive": true},
                                {"id": 3, "parent": 1, "tag": "div", "domId": "sticky-footer", "position": "fixed"}
                            ]
                        })
                    } else {
                        json!({
                            "target": 2,
                            "stack": [2, 1, 0],
                            "nodes": [
                                {"id": 0, "parent": null, "tag": "html", "landmarkOrChrome": true},
                                {"id": 1, "parent": 0, "tag": "body", "landmarkOrChrome": true},
                                {"id": 2, "parent": 1, "tag": "button", "domId": "reply-btn", "role": "button", "name": "Reply", "interactive": true}
                            ]
                        })
                    };
                    json!({"result": {"value": scene}})
                }
                _ => json!({}),
            })
        }
    }

    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab",
        Arc::new(tokio::sync::OnceCell::new()),
        Default::default(),
    );
    let (closed, _) = watch::channel(None);
    let connection = Arc::new(StickyFooterConnection {
        closed,
        subscribers: Mutex::new(Vec::new()),
        probed_points: Mutex::new(Vec::new()),
        input_commands: Mutex::new(Vec::new()),
    });
    let page = CdpClient::from_connection(connection.clone())
        .attach_page("main")
        .await
        .unwrap();
    runtime.shared.bind(&page);

    runtime.click_node(&page, 11).await.unwrap();

    let probed = connection.probed_points.lock().clone();
    assert_eq!(probed.len(), 5);
    assert!((probed[0].0 - 742.0).abs() < 1e-9 && (probed[0].1 - 856.0).abs() < 1e-9);
    assert!((probed[3].0 - 742.0).abs() < 1e-9 && (probed[3].1 - 840.0).abs() < 1e-9);

    let inputs = connection.input_commands.lock().clone();
    assert_eq!(inputs.len(), 3);
    for (method, params) in &inputs {
        assert_eq!(method, "Input.dispatchMouseEvent");
        assert!((params["x"].as_f64().unwrap() - 742.0).abs() < 1e-9);
        assert!((params["y"].as_f64().unwrap() - 840.0).abs() < 1e-9);
    }
}
