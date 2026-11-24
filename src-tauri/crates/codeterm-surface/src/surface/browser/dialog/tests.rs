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
fn beforeunload_during_agent_navigation_is_accepted_without_blocking_human() {
    let mut state = DialogState::default();
    assert_eq!(state.begin_agent_navigation(), NavigationStartPlan::Proceed);
    assert_eq!(
        state.on_dialog_opening(dialog("beforeunload", "")),
        DialogOpeningDecision::AcceptForAgentNavigation
    );
    assert!(!state.is_blocking());
    assert_eq!(state.active(), None);
    state.end_agent_navigation();
    assert_eq!(
        navigation_outcome("navigation loaded: https://example.test/next".into(), true),
        "navigation loaded: https://example.test/next (beforeunload dialog accepted)"
    );
    assert_eq!(
        navigation_outcome("navigation loaded: https://example.test/next".into(), false),
        "navigation loaded: https://example.test/next"
    );
}

#[test]
fn page_raised_alert_confirm_and_prompt_always_block_even_during_navigation() {
    for kind in ["alert", "confirm", "prompt"] {
        let mut state = DialogState::default();
        assert_eq!(state.begin_agent_navigation(), NavigationStartPlan::Proceed);
        let opened = dialog(kind, "Are you sure?");
        assert_eq!(
            state.on_dialog_opening(opened.clone()),
            DialogOpeningDecision::BlockForHuman
        );
        assert!(state.is_blocking());
        assert_eq!(state.active(), Some(&opened));
        state.end_agent_navigation();

        // Subsequent navigation and non-navigation verbs both refuse without dismissing it.
        assert_eq!(
            state.begin_agent_navigation(),
            NavigationStartPlan::Blocked(opened.clone())
        );
        for verb in ["goto", "reload", "back", "forward", "snapshot", "click"] {
            let error = state.check_page_verb(verb).unwrap_err();
            assert!(error.contains(kind), "{verb}: {error}");
            assert!(error.contains("Take control"), "{verb}: {error}");
        }

        state.on_dialog_closed();
        assert!(!state.is_blocking());
        assert!(state.check_page_verb("snapshot").is_ok());
    }
}

#[test]
fn pre_existing_beforeunload_blocks_other_verbs_but_navigation_verbs_can_resolve_it() {
    let mut state = DialogState::default();
    let opened = dialog("beforeunload", "Unsaved draft");
    assert_eq!(
        state.on_dialog_opening(opened),
        DialogOpeningDecision::BlockForHuman
    );
    assert!(state.is_blocking());
    let snap_err = state.check_page_verb("snapshot").unwrap_err();
    assert!(snap_err.contains("beforeunload"));
    assert!(snap_err.contains("goto"));
    for nav_verb in ["goto", "reload", "back", "forward"] {
        assert!(state.check_page_verb(nav_verb).is_ok(), "{nav_verb}");
    }
    assert_eq!(
        state.begin_agent_navigation(),
        NavigationStartPlan::DismissStaleBeforeUnloadThenProceed
    );
    state.on_dialog_closed();
    assert!(!state.is_blocking());
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
    assert!(is_no_dialog_cdp_error(
        "CDP error -32000: No dialog is showing"
    ));
    assert!(!is_no_dialog_cdp_error("CDP connection closed"));
}

#[derive(Clone, Copy)]
enum NavDialogBehavior {
    BeforeUnload,
    Alert,
    Clean,
}

struct NavFakeConnection {
    behavior: NavDialogBehavior,
    closed: tokio::sync::watch::Sender<Option<String>>,
    subscribers: parking_lot::Mutex<
        Vec<(
            codeterm_browser::cdp_wire::EventFilter,
            tokio::sync::mpsc::Sender<std::sync::Arc<codeterm_browser::cdp_wire::CdpEvent>>,
        )>,
    >,
    requests: parking_lot::Mutex<Vec<(Option<String>, String, serde_json::Value)>>,
    beforeunload_accepted: tokio::sync::watch::Sender<bool>,
}

impl NavFakeConnection {
    fn new(behavior: NavDialogBehavior) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            behavior,
            closed: tokio::sync::watch::channel(None).0,
            subscribers: parking_lot::Mutex::new(Vec::new()),
            requests: parking_lot::Mutex::new(Vec::new()),
            beforeunload_accepted: tokio::sync::watch::channel(false).0,
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
}

#[async_trait::async_trait]
impl codeterm_browser::cdp::CdpConnection for NavFakeConnection {
    fn endpoint(&self) -> &str {
        "ws://127.0.0.1:1/devtools/browser/nav-dialog"
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
        deadline: std::time::Duration,
    ) -> Result<serde_json::Value, codeterm_browser::CdpError> {
        self.requests
            .lock()
            .push((session.map(str::to_string), method.into(), params.clone()));
        match method {
            "Target.attachToTarget" => Ok(json!({ "sessionId": "ST1" })),
            "Page.getFrameTree" => Ok(json!({
                "frameTree": { "frame": { "id": "T1", "loaderId": "L1" } }
            })),
            "Page.handleJavaScriptDialog" => {
                let accept = params["accept"].as_bool().unwrap_or(false);
                self.emit(
                    "Page.javascriptDialogClosed",
                    Some("ST1"),
                    json!({ "result": accept, "userInput": "" }),
                );
                if accept {
                    self.beforeunload_accepted.send_replace(true);
                }
                Ok(json!({}))
            }
            "Page.navigate" => {
                let url = params["url"].as_str().unwrap_or("https://next.test/");
                match self.behavior {
                    NavDialogBehavior::BeforeUnload => {
                        let mut accepted = self.beforeunload_accepted.subscribe();
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "beforeunload", "message": "", "url": "https://dirty.test/" }),
                        );
                        tokio::time::timeout(deadline, accepted.wait_for(|done| *done))
                            .await
                            .map_err(|_| {
                                codeterm_browser::CdpError::Deadline("Page.navigate".into())
                            })?
                            .map_err(|_| {
                                codeterm_browser::CdpError::Closed("dialog channel closed".into())
                            })?;
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
                    NavDialogBehavior::Alert => {
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "alert", "message": "Stop now", "url": url }),
                        );
                        Ok(json!({ "frameId": "T1", "loaderId": "L2" }))
                    }
                    NavDialogBehavior::Clean => {
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
    behavior: NavDialogBehavior,
) -> (
    tempfile::TempDir,
    super::super::BrowserRuntime,
    codeterm_browser::PageChannel,
    std::sync::Arc<NavFakeConnection>,
) {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::super::BrowserRuntime::new(
        codeterm_browser::BrowserHostConfig::under_data_dir(root.path()),
        "Orla",
        "tab-id",
        std::sync::Arc::new(tokio::sync::OnceCell::new()),
        crate::surface::runtime::SurfaceResume::default(),
    );
    let conn = NavFakeConnection::new(behavior);
    let client = codeterm_browser::CdpClient::from_connection(conn.clone());
    let page = client.attach_page("T1").await.unwrap();
    runtime.shared.bind(&page);
    (root, runtime, page, conn)
}

#[tokio::test]
async fn agent_goto_resolves_its_own_beforeunload_and_reports_outcome() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(NavDialogBehavior::BeforeUnload).await;
    let outcome = runtime
        .navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/done"),
        )
        .await
        .unwrap();
    assert_eq!(
        outcome,
        "navigation loaded: https://next.test/done (beforeunload dialog accepted)"
    );
    assert_eq!(runtime.shared.state().needs_human, "none");
    let dialog_calls: Vec<_> = conn
        .requests
        .lock()
        .iter()
        .filter(|(_, method, _)| method == "Page.handleJavaScriptDialog")
        .cloned()
        .collect();
    assert_eq!(dialog_calls.len(), 1);
    assert_eq!(dialog_calls[0].2["accept"], true);
}

#[tokio::test]
async fn agent_goto_never_dismisses_alert_and_blocks_promptly() {
    let (_root, runtime, page, conn) = runtime_with_fake_page(NavDialogBehavior::Alert).await;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/alert"),
        ),
    )
    .await
    .expect("alert dialog must surface promptly, not wait for completion deadline")
    .unwrap_err();
    assert!(error.contains("alert"), "{error}");
    assert!(error.contains("Stop now"), "{error}");
    assert!(error.contains("Take control"), "{error}");
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    assert!(runtime.status_text().contains("dialog: alert — Stop now"));
    assert!(
        !conn
            .requests
            .lock()
            .iter()
            .any(|(_, method, _)| method == "Page.handleJavaScriptDialog"),
        "alert must never be auto-dismissed"
    );

    // Follow-up goto refuses immediately without sending another Page.navigate.
    let nav_count_before = conn
        .requests
        .lock()
        .iter()
        .filter(|(_, method, _)| method == "Page.navigate")
        .count();
    let follow_up = runtime
        .navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/other"),
        )
        .await
        .unwrap_err();
    assert!(follow_up.contains("blocked:"), "{follow_up}");
    let nav_count_after = conn
        .requests
        .lock()
        .iter()
        .filter(|(_, method, _)| method == "Page.navigate")
        .count();
    assert_eq!(nav_count_before, nav_count_after);
}

#[tokio::test]
async fn agent_goto_clears_pre_existing_beforeunload_then_navigates() {
    let (_root, runtime, page, conn) = runtime_with_fake_page(NavDialogBehavior::Clean).await;
    runtime
        .shared
        .record_blocking_dialog(page.session_id(), dialog("beforeunload", "Unsaved text"));
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    let outcome = runtime
        .navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/clean"),
        )
        .await
        .unwrap();
    assert_eq!(outcome, "navigation loaded: https://next.test/clean");
    assert_eq!(runtime.shared.state().needs_human, "none");
    let dialog_calls: Vec<_> = conn
        .requests
        .lock()
        .iter()
        .filter(|(_, method, _)| method == "Page.handleJavaScriptDialog")
        .cloned()
        .collect();
    assert_eq!(dialog_calls.len(), 1);
    assert_eq!(dialog_calls[0].2["accept"], false);
}
