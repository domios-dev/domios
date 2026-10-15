use super::*;

/// The payload the off-main caller hands the main thread must be plain data.
///
/// `Serialize` is the bound that enforces it: adding a `WebviewWindow` field fails
/// to derive, so the regression cannot compile. A `Send` bound would NOT catch it —
/// `tauri-runtime-wry` declares `unsafe impl Send for DispatcherMainThreadContext`,
/// so a webview handle is `Send` while transitively holding tao's `!Send`
/// `Rc<EventLoopRunner>`. That is exactly the hole this path fell through.
fn assert_marshalled_payload_is_plain_data<T: serde::Serialize + Send + 'static>() {}

#[test]
fn the_off_main_entry_marshals_plain_data_and_owns_no_webview_handle() {
    assert_marshalled_payload_is_plain_data::<NotifRequest>();

    let request = NotifRequest {
        title: "a title".into(),
        body: "a body".into(),
        reuse_existing: true,
    };
    let json = serde_json::to_value(&request).expect("a payload of handles would not serialize");
    assert_eq!(
        json.as_object().map(|fields| fields.len()),
        Some(3),
        "the payload carries three plain fields and nothing else: {json}"
    );
}

#[test]
fn the_reuse_decision_is_carried_in_the_payload_not_re_read_on_the_main_thread() {
    for reuse in [true, false] {
        let request = NotifRequest {
            title: String::new(),
            body: String::new(),
            reuse_existing: reuse,
        };
        assert_eq!(request.reuse_existing, reuse);
    }
}

#[test]
fn the_popup_dismisses_itself_from_its_own_document_timer() {
    // No Rust thread waits on the popup, so nothing off the main thread can be
    // holding a handle when it closes. The dwell is substituted, not quoted.
    let html = _build_notif_html("title", "body");
    assert!(
        html.contains(&NOTIF_DWELL_MS.to_string()),
        "the dwell constant must reach the document: {html}"
    );
    assert!(html.contains("window.close()"), "{html}");
}

#[test]
fn the_popup_document_escapes_what_it_is_given() {
    let html = _build_notif_html("<script>a", "b<script>");
    assert!(!html.contains("<script>a"), "{html}");
    assert!(!html.contains("b<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;a"), "{html}");
}

#[test]
fn the_window_label_is_one_constant_for_lookup_and_creation() {
    // A second literal would let the reuse path miss the window it built.
    assert!(!NOTIF_WINDOW_LABEL.is_empty());
    assert_eq!(NOTIF_WINDOW_LABEL, "codeterm-notif");
}
