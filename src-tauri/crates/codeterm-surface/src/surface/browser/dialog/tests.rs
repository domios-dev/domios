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
    for kind in ["alert", "confirm", "prompt", "beforeunload"] {
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
    let close_err = runtime.agent_close().await.unwrap_err();
    match close_err {
        SurfaceError::Blocked(detail) => {
            assert!(
                detail.contains("dialog: beforeunload — Owner draft"),
                "{detail}"
            );
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
