use serde_json::json;

use super::*;

fn dialog(kind: &str, message: &str) -> ActiveDialog {
    ActiveDialog::from_cdp_params(&json!({
        "type": kind,
        "message": message,
        "url": "https://example.test/form",
    }))
}

#[test]
fn all_dialogs_including_beforeunload_block_and_format_status_and_error() {
    for kind in ["alert", "confirm", "prompt", "auth", "beforeunload"] {
        let mut state = DialogState::default();
        let opened = dialog(kind, "Unsaved changes");
        state.on_dialog_opening(opened.clone());
        assert!(state.is_blocking());
        assert_eq!(state.active(), Some(&opened));
        assert_eq!(
            opened.status_line(),
            format!("dialog: {kind} — Unsaved changes")
        );

        let error = state.check_page_verb().unwrap_err();
        assert!(
            error.contains(&format!("dialog: {kind} — Unsaved changes")),
            "{error}"
        );
        assert!(error.contains("needs human: dialog"), "{error}");
        assert!(error.contains("Take control"), "{error}");
        if kind == "beforeunload" {
            assert!(
                error.contains("domios plugin browser close --discard-unsaved"),
                "{error}"
            );
        } else {
            assert!(
                !error.contains("--discard-unsaved"),
                "non-beforeunload dialog must not suggest discard: {error}"
            );
        }

        state.on_dialog_closed();
        assert!(!state.is_blocking());
        assert!(state.check_page_verb().is_ok());
    }
}

#[test]
fn committed_navigation_clears_beforeunload_but_not_alert() {
    let mut state = DialogState::default();
    state.on_dialog_opening(dialog("beforeunload", "Leave?"));
    state.on_navigated();
    assert!(!state.is_blocking());

    let alert = dialog("alert", "Still here");
    state.on_dialog_opening(alert.clone());
    state.on_navigated();
    assert_eq!(state.active(), Some(&alert));
}

#[test]
fn dialog_params_normalize_whitespace_and_bound_long_messages() {
    let long = format!("  line1\n\t{}  ", "a".repeat(MAX_DIALOG_MESSAGE_CHARS + 50));
    let parsed = dialog("alert", &long);
    assert_eq!(parsed.kind, DialogKind::Alert);
    assert!(parsed.message.starts_with("line1 "));
    assert!(parsed.message.ends_with('…'));
    assert_eq!(parsed.message.chars().count(), MAX_DIALOG_MESSAGE_CHARS + 1);
    assert!(parsed.status_line().starts_with("dialog: alert — line1 "));

    let empty = dialog("beforeunload", "");
    assert_eq!(empty.status_line(), "dialog: beforeunload");
    assert!(empty.blocking_error().contains("dialog: beforeunload"));
}

#[derive(Clone, Copy)]
enum DialogBehavior {
    BeforeUnload,
    Alert,
    DialogDuringHistoryFetch,
    StallFirstNetworkEnable,
    StallFirstOverlayEval,
    StallScreenshotAfterSuppress,
    Clean,
}

struct DialogFakeConnection {
    behavior: DialogBehavior,
    closed: tokio::sync::watch::Sender<Option<String>>,
    subscribers: parking_lot::Mutex<
        Vec<(
            codeterm_browser::cdp_wire::EventFilter,
            tokio::sync::mpsc::Sender<std::sync::Arc<codeterm_browser::cdp_wire::CdpEvent>>,
        )>,
    >,
    requests: parking_lot::Mutex<Vec<(Option<String>, String, serde_json::Value)>>,
    network_enable_started: tokio::sync::watch::Sender<bool>,
    network_enable_calls: std::sync::atomic::AtomicUsize,
    overlay_eval_started: tokio::sync::watch::Sender<bool>,
    overlay_eval_calls: std::sync::atomic::AtomicUsize,
    screenshot_started: tokio::sync::watch::Sender<bool>,
    capture_true_calls: std::sync::atomic::AtomicUsize,
    capture_false_calls: std::sync::atomic::AtomicUsize,
    capture_false_done: tokio::sync::watch::Sender<bool>,
}

impl DialogFakeConnection {
    fn new(behavior: DialogBehavior) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            behavior,
            closed: tokio::sync::watch::channel(None).0,
            subscribers: parking_lot::Mutex::new(Vec::new()),
            requests: parking_lot::Mutex::new(Vec::new()),
            network_enable_started: tokio::sync::watch::channel(false).0,
            network_enable_calls: std::sync::atomic::AtomicUsize::new(0),
            overlay_eval_started: tokio::sync::watch::channel(false).0,
            overlay_eval_calls: std::sync::atomic::AtomicUsize::new(0),
            screenshot_started: tokio::sync::watch::channel(false).0,
            capture_true_calls: std::sync::atomic::AtomicUsize::new(0),
            capture_false_calls: std::sync::atomic::AtomicUsize::new(0),
            capture_false_done: tokio::sync::watch::channel(false).0,
        })
    }

    fn emit(&self, method: &str, session: Option<&str>, params: serde_json::Value) {
        let event = std::sync::Arc::new(codeterm_browser::cdp_wire::CdpEvent {
            method: method.into(),
            session_id: session.map(str::to_string),
            params,
        });
        self.subscribers.lock().retain(|(filter, sender)| {
            if filter.matches(&event) {
                let _ = sender.try_send(event.clone());
            }
            !sender.is_closed()
        });
    }

    fn count(&self, method: &str) -> usize {
        self.requests
            .lock()
            .iter()
            .filter(|(_, sent, _)| sent == method)
            .count()
    }
}

#[async_trait::async_trait]
impl codeterm_browser::cdp::CdpConnection for DialogFakeConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/devtools/browser/dialog"
    }
    fn closed(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.closed.subscribe()
    }
    fn close(&self, reason: &str) {
        self.closed.send_replace(Some(reason.into()));
        self.subscribers.lock().clear();
    }
    fn subscribe(
        &self,
        filter: codeterm_browser::cdp_wire::EventFilter,
    ) -> codeterm_browser::EventStream {
        let (sender, receiver) = tokio::sync::mpsc::channel(64);
        self.subscribers.lock().push((filter, sender));
        codeterm_browser::EventStream::from_receiver(receiver)
    }
    async fn send_to(
        &self,
        session: Option<&str>,
        method: &str,
        params: serde_json::Value,
        _deadline: std::time::Duration,
    ) -> Result<serde_json::Value, codeterm_browser::CdpError> {
        self.requests
            .lock()
            .push((session.map(str::to_string), method.into(), params.clone()));
        match method {
            "Target.attachToTarget" => Ok(json!({ "sessionId": "ST1" })),
            "Page.getFrameTree" => Ok(json!({
                "frameTree": { "frame": { "id": "T1", "loaderId": "L1" } }
            })),
            "Page.getNavigationHistory" => {
                if matches!(self.behavior, DialogBehavior::DialogDuringHistoryFetch) {
                    self.emit(
                        "Page.javascriptDialogOpening",
                        Some("ST1"),
                        json!({ "type": "confirm", "message": "Discard form?", "url": "https://dirty.test/" }),
                    );
                    std::future::pending::<()>().await;
                }
                Ok(json!({ "currentIndex": 0, "entries": [{ "id": 1, "url": "https://a.test/" }] }))
            }
            "Network.enable" => {
                let call = self
                    .network_enable_calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if matches!(self.behavior, DialogBehavior::StallFirstNetworkEnable) && call == 0 {
                    self.network_enable_started.send_replace(true);
                    std::future::pending::<()>().await;
                }
                Ok(json!({}))
            }
            "Runtime.evaluate" => {
                let expr = params["expression"].as_str().unwrap_or("");
                if expr.contains("capture(true)") {
                    self.capture_true_calls
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return Ok(json!({ "result": { "value": null } }));
                }
                if expr.contains("capture(false)") {
                    self.capture_false_calls
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    self.capture_false_done.send_replace(true);
                    return Ok(json!({ "result": { "value": null } }));
                }
                if expr.contains("domios.browser.target") {
                    let call = self
                        .overlay_eval_calls
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if matches!(self.behavior, DialogBehavior::StallFirstOverlayEval) && call == 0 {
                        self.overlay_eval_started.send_replace(true);
                        std::future::pending::<()>().await;
                    }
                }
                Ok(json!({ "result": { "value": null } }))
            }
            "Page.getLayoutMetrics" => Ok(json!({
                "cssLayoutViewport": { "pageX": 0, "pageY": 0, "clientWidth": 1280, "clientHeight": 720 }
            })),
            "Page.captureScreenshot" => {
                if matches!(self.behavior, DialogBehavior::StallScreenshotAfterSuppress) {
                    self.screenshot_started.send_replace(true);
                    std::future::pending::<()>().await;
                }
                Ok(json!({ "data": "QQ==" }))
            }
            "Page.navigate" => {
                let url = params["url"].as_str().unwrap_or("https://next.test/");
                match self.behavior {
                    DialogBehavior::BeforeUnload => {
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "beforeunload", "message": "Unsaved draft", "url": "https://dirty.test/" }),
                        );
                        std::future::pending::<()>().await;
                        unreachable!()
                    }
                    DialogBehavior::Alert => {
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "alert", "message": "Stop now", "url": url }),
                        );
                        Ok(json!({ "frameId": "T1", "loaderId": "L2" }))
                    }
                    DialogBehavior::Clean
                    | DialogBehavior::StallFirstNetworkEnable
                    | DialogBehavior::StallFirstOverlayEval
                    | DialogBehavior::StallScreenshotAfterSuppress
                    | DialogBehavior::DialogDuringHistoryFetch => {
                        self.emit(
                            "Page.frameNavigated",
                            Some("ST1"),
                            json!({ "frame": { "id": "T1", "loaderId": "L2", "url": url } }),
                        );
                        self.emit(
                            "Page.lifecycleEvent",
                            Some("ST1"),
                            json!({ "frameId": "T1", "loaderId": "L2", "name": "load" }),
                        );
                        Ok(json!({ "frameId": "T1", "loaderId": "L2" }))
                    }
                }
            }
            _ => Ok(json!({})),
        }
    }
}

async fn runtime_with_fake_page(
    behavior: DialogBehavior,
) -> (
    tempfile::TempDir,
    super::super::BrowserRuntime,
    codeterm_browser::PageChannel,
    std::sync::Arc<DialogFakeConnection>,
) {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::super::BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Orla",
        "tab-id",
        std::sync::Arc::new(tokio::sync::OnceCell::new()),
        crate::surface::runtime::SurfaceResume::default(),
    );
    let conn = DialogFakeConnection::new(behavior);
    let client = codeterm_browser::CdpClient::from_connection(conn.clone());
    let page = client.attach_page("T1").await.unwrap();
    runtime.shared.bind(&page);
    (root, runtime, page, conn)
}

#[tokio::test]
async fn agent_goto_fails_fast_on_beforeunload_and_alert_without_answering() {
    for (behavior, expected_line) in [
        (
            DialogBehavior::BeforeUnload,
            "dialog: beforeunload — Unsaved draft",
        ),
        (DialogBehavior::Alert, "dialog: alert — Stop now"),
    ] {
        let (_root, runtime, page, conn) = runtime_with_fake_page(behavior).await;
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            runtime.navigate(
                &page,
                super::super::input::NavAction::Goto,
                Some("https://next.test/target"),
            ),
        )
        .await
        .expect("dialog must fail navigation promptly, never waiting for the 15s deadline")
        .unwrap_err();
        assert!(error.contains(expected_line), "{error}");
        assert!(error.contains("Take control"), "{error}");
        assert_eq!(runtime.shared.state().needs_human, "dialog");
        assert!(runtime.status_text().contains(expected_line));
        assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);

        let nav_before = conn.count("Page.navigate");
        let follow_up = runtime
            .navigate(
                &page,
                super::super::input::NavAction::Goto,
                Some("https://next.test/other"),
            )
            .await
            .unwrap_err();
        assert!(follow_up.contains(expected_line), "{follow_up}");
        assert_eq!(conn.count("Page.navigate"), nav_before);
        assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    }
}

#[tokio::test]
async fn page_verb_fails_fast_when_dialog_opens_mid_action() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(DialogBehavior::DialogDuringHistoryFetch).await;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.navigate(&page, super::super::input::NavAction::Back, None),
    )
    .await
    .expect("mid-verb dialog must fail promptly")
    .unwrap_err();
    assert!(error.contains("dialog: confirm — Discard form?"), "{error}");
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    assert!(runtime
        .status_text()
        .contains("dialog: confirm — Discard form?"));

    // Arbitrary stalled page verb raced through `race_with_dialog` also fails fast.
    let (_root2, runtime2, _page2, conn2) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let verb = runtime2.race_with_dialog(std::future::pending::<Result<String, String>>());
    tokio::pin!(verb);
    conn2.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "prompt", "message": "Enter PIN", "url": "https://a.test/" }),
    );
    let err = tokio::time::timeout(std::time::Duration::from_secs(2), verb)
        .await
        .expect("mid-verb prompt must abort stalled page verb promptly")
        .unwrap_err();
    assert!(err.contains("dialog: prompt — Enter PIN"), "{err}");
    assert_eq!(conn2.count("Page.handleJavaScriptDialog"), 0);
}

#[tokio::test]
async fn agent_close_and_open_profile_refuse_pre_existing_dialog_without_unbind_or_close() {
    use crate::surface::model::SurfaceError;
    use crate::surface::runtime::SurfaceRuntime;

    let (_root, runtime, _page, conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let mut dialogs = runtime.shared.subscribe_dialogs();
    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "beforeunload", "message": "Owner draft", "url": "https://owner.test/" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|snapshot| snapshot.dialog.is_some()),
    )
    .await
    .unwrap()
    .unwrap();

    // Agent close must refuse before unbinding or tearing down the target.
    let owner_ctx = call_ctx(_root.path(), Some("tab-id"), Some("tab-id"), None);
    let close_err = runtime
        .agent_close(&owner_ctx, crate::surface::runtime::AgentCloseMode::Normal)
        .await
        .unwrap_err();
    match close_err {
        SurfaceError::Blocked(detail) => {
            assert!(
                detail.contains("dialog: beforeunload — Owner draft"),
                "{detail}"
            );
            assert!(
                detail.contains("domios plugin browser close --discard-unsaved"),
                "{detail}"
            );
            assert!(detail.contains("Take control"), "{detail}");
        }
        other => panic!("expected SurfaceError::Blocked, got {other:?}"),
    }

    // Agent open --profile and open <url> must also refuse without unbind/host teardown.
    let profile_err = runtime
        .open_verb(&["--profile".into(), "Work".into()])
        .await
        .unwrap_err();
    assert!(
        profile_err.contains("dialog: beforeunload — Owner draft"),
        "{profile_err}"
    );
    let url_err = runtime
        .open_verb(&["https://other.test/".into()])
        .await
        .unwrap_err();
    assert!(
        url_err.contains("dialog: beforeunload — Owner draft"),
        "{url_err}"
    );

    assert_eq!(conn.count("Target.closeTarget"), 0);
    assert_eq!(conn.count("Browser.close"), 0);
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    assert_eq!(conn.count("Page.navigate"), 0);
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    assert!(runtime
        .status_text()
        .contains("dialog: beforeunload — Owner draft"));

    // Listener must still be bound: closing the dialog on the page clears needs_human.
    conn.emit(
        "Page.javascriptDialogClosed",
        Some("ST1"),
        json!({ "result": false, "userInput": "" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|snapshot| snapshot.dialog.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(runtime.shared.state().needs_human, "none");

    // Owner close authority stays separate and succeeds even with a dialog open.
    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "alert", "message": "Owner closing", "url": "https://owner.test/" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|snapshot| snapshot.dialog.is_some()),
    )
    .await
    .unwrap()
    .unwrap();
    runtime.close().await.unwrap();
}

#[tokio::test]
async fn cancelling_domain_setup_before_network_enable_allows_clean_reentry() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(DialogBehavior::StallFirstNetworkEnable).await;
    let mut started = conn.network_enable_started.subscribe();
    {
        let setup = runtime.ensure_page_domains(&page);
        tokio::pin!(setup);
        tokio::select! {
            _ = &mut setup => panic!("first Network.enable should stall"),
            ready = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                started.wait_for(|hit| *hit),
            ) => {
                ready.expect("Network.enable should be reached").unwrap();
            }
        }
    }
    assert!(
        runtime.shared.needs_domains_setup(&page),
        "cancelled setup must not commit domains_ready"
    );
    runtime.ensure_page_domains(&page).await.unwrap();
    assert!(!runtime.shared.needs_domains_setup(&page));

    let runtime_enables = conn.count("Runtime.enable");
    let network_enables = conn.count("Network.enable");
    runtime.ensure_page_domains(&page).await.unwrap();
    assert_eq!(conn.count("Runtime.enable"), runtime_enables);
    assert_eq!(conn.count("Network.enable"), network_enables);
}

#[tokio::test]
async fn race_with_dialog_prefers_opening_on_simultaneous_ready_and_final_poll() {
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

    // Subcase A: verb future publishes an opening on its final poll and returns Ready(Ok).
    struct OpenOnFinalPoll {
        shared: Arc<super::super::page::PageShared>,
    }
    impl Future for OpenOnFinalPoll {
        type Output = Result<&'static str, String>;
        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.shared
                .inject_dialog_for_test("ST1", dialog("alert", "Final poll"));
            Poll::Ready(Ok("should not win"))
        }
    }

    let (_root, runtime, _page, _conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let err = runtime
        .race_with_dialog(OpenOnFinalPoll {
            shared: runtime.shared.clone(),
        })
        .await
        .unwrap_err();
    assert!(err.contains("dialog: alert — Final poll"), "{err}");

    // Subcase B: both `dialogs.changed()` and the verb future become ready on the exact same poll.
    struct ManualReady {
        ready: Arc<std::sync::atomic::AtomicBool>,
        waker: Arc<parking_lot::Mutex<Option<Waker>>>,
    }
    impl Future for ManualReady {
        type Output = Result<&'static str, String>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.ready.load(std::sync::atomic::Ordering::SeqCst) {
                Poll::Ready(Ok("should not win"))
            } else {
                *self.waker.lock() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    let (_root2, runtime2, _page2, _conn2) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let waker = Arc::new(parking_lot::Mutex::new(None));
    let race = runtime2.race_with_dialog(ManualReady {
        ready: ready.clone(),
        waker: waker.clone(),
    });
    tokio::pin!(race);
    assert!(futures_util::poll!(&mut race).is_pending());
    // Make BOTH the verb future and the dialog watch ready before the next poll.
    ready.store(true, std::sync::atomic::Ordering::SeqCst);
    runtime2
        .shared
        .inject_dialog_for_test("ST1", dialog("confirm", "Simultaneous"));
    if let Some(w) = waker.lock().take() {
        w.wake();
    }
    let err2 = race.await.unwrap_err();
    assert!(err2.contains("dialog: confirm — Simultaneous"), "{err2}");
}

#[tokio::test]
async fn dialog_opening_during_overlay_evaluation_fails_fast_and_allows_reentry() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(DialogBehavior::StallFirstOverlayEval).await;
    let mut overlay_started = conn.overlay_eval_started.subscribe();
    let mut dialogs = runtime.shared.subscribe_dialogs();

    let prep = runtime.race_with_dialog(async {
        let _launch = runtime.launch_gate.lock().await;
        runtime.ensure_page_domains(&page).await
    });
    tokio::pin!(prep);
    tokio::select! {
        result = &mut prep => panic!("overlay eval should stall at protocol barrier: {result:?}"),
        hit = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            overlay_started.wait_for(|started| *started),
        ) => {
            hit.expect("overlay Runtime.evaluate must be reached").unwrap();
        }
    }

    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "alert", "message": "Overlay blocked", "url": "https://a.test/" }),
    );
    let err = tokio::time::timeout(std::time::Duration::from_secs(2), prep)
        .await
        .expect("dialog during overlay setup must fail fast, not wait for 30s CDP deadline")
        .unwrap_err();
    assert!(err.contains("dialog: alert — Overlay blocked"), "{err}");
    assert!(
        runtime.shared.needs_domains_setup(&page),
        "aborted setup must not commit domains_ready"
    );
    assert!(
        runtime.launch_gate.try_lock().is_ok(),
        "launch_gate must be released on cancellation"
    );
    assert!(
        runtime.overlay_session.try_lock().is_ok(),
        "overlay_session must be released on cancellation"
    );

    // After dialog resolution, re-entering setup completes cleanly and commits domains_ready.
    conn.emit(
        "Page.javascriptDialogClosed",
        Some("ST1"),
        json!({ "result": false, "userInput": "" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|s| s.dialog.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    runtime.ensure_page_domains(&page).await.unwrap();
    assert!(!runtime.shared.needs_domains_setup(&page));
}

#[tokio::test]
async fn cancelling_after_overlay_suppression_ack_restores_presentation_once_after_dialog_closes() {
    use std::sync::atomic::Ordering;

    // Cancelled path: overlay_capture(true) is acked, then a dialog cancels capture_screenshot.
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(DialogBehavior::StallScreenshotAfterSuppress).await;
    let mut shot_started = conn.screenshot_started.subscribe();
    let mut restore_done = conn.capture_false_done.subscribe();

    let shot = runtime.race_with_dialog(runtime.capture_screenshot(
        &page,
        json!({ "format": "png", "captureBeyondViewport": false }),
        false,
    ));
    tokio::pin!(shot);
    tokio::select! {
        result = &mut shot => panic!("capture should stall after overlay suppression ack: {result:?}"),
        hit = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            shot_started.wait_for(|started| *started),
        ) => {
            hit.expect("Page.captureScreenshot must be reached after capture(true) ack").unwrap();
        }
    }
    assert_eq!(conn.capture_true_calls.load(Ordering::SeqCst), 1);
    assert_eq!(conn.capture_false_calls.load(Ordering::SeqCst), 0);

    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "alert", "message": "Mid-capture", "url": "https://a.test/" }),
    );
    let err = tokio::time::timeout(std::time::Duration::from_secs(2), shot)
        .await
        .expect("dialog mid-screenshot must fail fast")
        .unwrap_err();
    assert!(err.contains("dialog: alert — Mid-capture"), "{err}");
    assert_eq!(
        conn.capture_false_calls.load(Ordering::SeqCst),
        0,
        "cleanup must wait until the modal dialog closes before evaluating capture(false)"
    );

    // Resolving the dialog triggers exactly one capture(false) on the original page.
    conn.emit(
        "Page.javascriptDialogClosed",
        Some("ST1"),
        json!({ "result": false, "userInput": "" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        restore_done.wait_for(|done| *done),
    )
    .await
    .expect("overlay suppression must be restored after dialog resolution")
    .unwrap();
    assert_eq!(conn.capture_true_calls.load(Ordering::SeqCst), 1);
    assert_eq!(conn.capture_false_calls.load(Ordering::SeqCst), 1);

    // Normal non-cancelled path: balanced exactly once (no double capture(false) on Drop).
    let (_root2, runtime2, page2, conn2) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let ok = runtime2
        .capture_screenshot(
            &page2,
            json!({ "format": "png", "captureBeyondViewport": false }),
            false,
        )
        .await
        .unwrap();
    assert_eq!(ok["data"], "QQ==");
    assert_eq!(conn2.capture_true_calls.load(Ordering::SeqCst), 1);
    assert_eq!(conn2.capture_false_calls.load(Ordering::SeqCst), 1);
}

fn call_ctx(
    root: &std::path::Path,
    owner: Option<&str>,
    caller: Option<&str>,
    actor: Option<&str>,
) -> SurfaceCallCtx {
    SurfaceCallCtx {
        host: std::sync::Arc::new(crate::test_support::RecordingSurfaceHost::default()),
        owner_tab_id: owner.map(str::to_string),
        caller_tab_id: caller.map(str::to_string),
        data_dir: root.to_path_buf(),
        actor: actor.map(str::to_string),
    }
}

#[tokio::test]
async fn beforeunload_discard_closes_owned_session_without_dialog_answer_and_records_activity() {
    use crate::surface::runtime::{AgentCloseMode, SurfaceRuntime};

    let (root, runtime, _page, conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let mut dialogs = runtime.shared.subscribe_dialogs();
    let mut notices = runtime.shared.subscribe_notices();
    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({
            "type": "beforeunload",
            "message": "Unsaved reply draft",
            "url": "https://x.com/compose/post?draft=1#secret",
        }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|snapshot| snapshot.dialog.is_some()),
    )
    .await
    .unwrap()
    .unwrap();

    let owner_ctx = call_ctx(root.path(), Some("tab-id"), Some("tab-id"), None);
    let activity = runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap()
        .expect("discard must record an activity line");
    assert!(activity.contains("Orla"), "{activity}");
    assert!(activity.contains("https://x.com"), "{activity}");
    assert!(!activity.contains("secret"), "{activity}");
    assert!(activity.contains("beforeunload"), "{activity}");
    assert_eq!(notices.try_recv().unwrap().text, activity);
    assert_eq!(runtime.shared.state().activity, activity);
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    assert_eq!(runtime.shared.state().needs_human, "none");
    assert!(runtime.shared.active_dialog().is_none());
    assert!(runtime.shared.state().target_id.is_empty());
}

#[tokio::test]
async fn discard_refuses_non_beforeunload_dialogs_non_owner_and_owner_take_control() {
    use crate::surface::runtime::{AgentCloseMode, SurfaceRuntime};
    use codeterm_browser::viewport::ViewportRequest;

    for kind in ["alert", "confirm", "prompt", "auth"] {
        let (root, runtime, _page, conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
        let mut dialogs = runtime.shared.subscribe_dialogs();
        let mut notices = runtime.shared.subscribe_notices();
        conn.emit(
            "Page.javascriptDialogOpening",
            Some("ST1"),
            json!({ "type": kind, "message": "Human check", "url": "https://guard.test/form" }),
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            dialogs.wait_for(|snapshot| snapshot.dialog.is_some()),
        )
        .await
        .unwrap()
        .unwrap();

        let owner_ctx = call_ctx(root.path(), Some("tab-id"), Some("tab-id"), None);
        let err = runtime
            .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
            .await
            .unwrap_err();
        match err {
            SurfaceError::Blocked(detail) => {
                assert!(detail.contains(&format!("dialog: {kind}")), "{detail}");
                assert!(!detail.contains("--discard-unsaved"), "{detail}");
            }
            other => panic!("expected SurfaceError::Blocked for {kind}, got {other:?}"),
        }
        assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
        assert_eq!(conn.count("Target.closeTarget"), 0);
        assert_eq!(runtime.shared.state().needs_human, "dialog");
        assert!(notices.try_recv().is_err());
    }

    let (root, runtime, _page, conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let mut dialogs = runtime.shared.subscribe_dialogs();
    let mut notices = runtime.shared.subscribe_notices();
    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "beforeunload", "message": "Draft", "url": "https://dirty.test/reply" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|snapshot| snapshot.dialog.is_some()),
    )
    .await
    .unwrap()
    .unwrap();

    for non_owner in [
        call_ctx(root.path(), Some("tab-id"), Some("other-tab"), None),
        call_ctx(root.path(), Some("tab-id"), Some("tab-id"), Some("QA")),
        call_ctx(root.path(), None, Some("tab-id"), None),
    ] {
        assert!(matches!(
            runtime
                .agent_close(&non_owner, AgentCloseMode::DiscardUnsaved)
                .await,
            Err(SurfaceError::Blocked(_))
        ));
    }
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    assert!(runtime.shared.active_dialog().is_some());
    assert!(notices.try_recv().is_err());

    let owner_ctx = call_ctx(root.path(), Some("tab-id"), Some("tab-id"), None);
    let gen1 = runtime
        .viewport
        .lock()
        .await
        .join(
            "owner-viewer".into(),
            ViewportRequest {
                width: 1280.0,
                height: 720.0,
                dpr: 1.0,
            },
            false,
        )
        .unwrap();
    runtime
        .view_call(
            &owner_ctx,
            "viewport.claim",
            json!({ "viewer": "owner-viewer", "width": 1280.0, "height": 720.0, "dpr": 1.0 }),
        )
        .await
        .unwrap();
    match runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap_err()
    {
        SurfaceError::Blocked(detail) => assert!(detail.contains("Take control"), "{detail}"),
        other => panic!("expected Take control refusal, got {other:?}"),
    }
    assert!(runtime.shared.active_dialog().is_some());
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    assert!(notices.try_recv().is_err());

    // A stale viewer generation never revokes active control.
    runtime.release_control_if_generation("owner-viewer", gen1 + 1);
    assert!(runtime.owner_holds_control());

    runtime
        .view_call(
            &owner_ctx,
            "control.release",
            json!({ "viewer": "owner-viewer" }),
        )
        .await
        .unwrap();
    assert!(!runtime.owner_holds_control());
    let activity = runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap()
        .expect("discard succeeds once owner releases Take control");
    assert!(activity.contains("https://dirty.test"), "{activity}");
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
}

#[tokio::test]
async fn take_control_survives_interactive_reconnect_and_remount_without_claiming_size() {
    use crate::surface::runtime::{AgentCloseMode, SurfaceRuntime};
    use codeterm_browser::viewport::ViewportRequest;

    let (root, runtime, _page, conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
    let runtime = std::sync::Arc::new(runtime);
    let mut dialogs = runtime.shared.subscribe_dialogs();
    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "beforeunload", "message": "Draft", "url": "https://dirty.test/reply" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(|snapshot| snapshot.dialog.is_some()),
    )
    .await
    .unwrap()
    .unwrap();

    let owner_ctx = call_ctx(root.path(), Some("tab-id"), Some("tab-id"), None);

    // An unjoined viewer cannot record Take control.
    let early = runtime
        .view_call(
            &owner_ctx,
            "control.claim",
            json!({ "viewer": "unjoined-viewer" }),
        )
        .await
        .unwrap();
    assert_eq!(early.result["claimed"], false);
    assert!(!runtime.owner_holds_control());

    // First viewer establishes fixed viewport size without Take control.
    runtime
        .viewport
        .lock()
        .await
        .join(
            "desktop".into(),
            ViewportRequest {
                width: 1200.0,
                height: 800.0,
                dpr: 1.0,
            },
            false,
        )
        .unwrap();

    // Remount while interactive opens the stream with interactive=true: claims control, not size.
    let remount_stream = runtime
        .clone()
        .connect(
            Some(ViewportRequest {
                width: 640.0,
                height: 480.0,
                dpr: 1.0,
            }),
            Some("remount-viewer".into()),
            None,
            true,
        )
        .await;
    assert_eq!(
        runtime.viewport.lock().await.owner().as_deref(),
        Some("desktop")
    );
    assert!(runtime.owner_holds_control());
    match runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap_err()
    {
        SurfaceError::Blocked(detail) => assert!(detail.contains("Take control"), "{detail}"),
        other => panic!("expected Take control refusal on interactive remount, got {other:?}"),
    }

    // Stream drop clears the dropped generation; reconnect re-asserts control via size-less control.claim.
    drop(remount_stream);
    assert!(!runtime.owner_holds_control());

    let reconnect_stream = runtime
        .clone()
        .connect(
            Some(ViewportRequest {
                width: 640.0,
                height: 480.0,
                dpr: 1.0,
            }),
            Some("remount-viewer".into()),
            None,
            false,
        )
        .await;
    let reclaimed = runtime
        .view_call(
            &owner_ctx,
            "control.claim",
            json!({ "viewer": "remount-viewer" }),
        )
        .await
        .unwrap();
    assert_eq!(reclaimed.result["claimed"], true);
    assert_eq!(
        runtime.viewport.lock().await.owner().as_deref(),
        Some("desktop")
    );
    assert!(runtime.owner_holds_control());
    match runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap_err()
    {
        SurfaceError::Blocked(detail) => assert!(detail.contains("Take control"), "{detail}"),
        other => panic!("expected Take control refusal after reconnect, got {other:?}"),
    }

    drop(reconnect_stream);
    assert!(!runtime.owner_holds_control());
    let activity = runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap()
        .expect("discard succeeds once all interactive streams disconnect");
    assert!(activity.contains("https://dirty.test"), "{activity}");
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
}

#[tokio::test]
async fn discard_without_open_dialog_still_records_owner_and_origin_activity() {
    use crate::surface::runtime::{AgentCloseMode, SurfaceRuntime};

    let (root, runtime, _page, conn) = runtime_with_fake_page(DialogBehavior::Clean).await;
    runtime
        .shared
        .update(|state| state.url = "https://draft.test/editor?tab=1#hash".into());
    let mut notices = runtime.shared.subscribe_notices();

    let owner_ctx = call_ctx(root.path(), Some("tab-id"), Some("tab-id"), None);
    let activity = runtime
        .agent_close(&owner_ctx, AgentCloseMode::DiscardUnsaved)
        .await
        .unwrap()
        .expect("discard must always record an activity line");
    assert!(activity.contains("Orla"), "{activity}");
    assert!(activity.contains("https://draft.test"), "{activity}");
    assert!(!activity.contains("tab=1"), "{activity}");
    assert_eq!(notices.try_recv().unwrap().text, activity);
    assert_eq!(runtime.shared.state().activity, activity);
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
}
