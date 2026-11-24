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
fn beforeunload_during_agent_navigation_is_tracked_and_surfaces_if_cancelled() {
    let mut state = DialogState::default();
    let token = state.begin_agent_navigation().unwrap();
    let tracked = state.on_dialog_opening(dialog("beforeunload", "Unsaved draft"));
    assert_eq!(tracked.generation, 1);
    assert_eq!(tracked.owned_by_nav, Some(token));
    assert!(!state.is_blocking_for_human());
    assert!(state.can_accept_beforeunload(token, tracked.generation));

    // Cancellation before dialog close leaves an actionable blocking state.
    assert!(state.end_agent_navigation(token));
    assert!(state.is_blocking_for_human());
    assert!(!state.can_accept_beforeunload(token, tracked.generation));
    assert_eq!(
        state.active_blocking().map(ActiveDialog::status_line),
        Some("dialog: beforeunload — Unsaved draft".into())
    );
    assert!(state.begin_agent_navigation().is_err());
    assert!(state.check_page_verb().is_err());

    state.on_dialog_closed();
    assert!(!state.is_blocking_for_human());
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
fn superseded_beforeunload_generation_cannot_authorize_answering_replacement_dialog() {
    let mut state = DialogState::default();
    let token = state.begin_agent_navigation().unwrap();
    let first = state.on_dialog_opening(dialog("beforeunload", ""));
    assert!(state.can_accept_beforeunload(token, first.generation));

    state.on_dialog_closed();
    assert!(!state.can_accept_beforeunload(token, first.generation));

    let second = state.on_dialog_opening(dialog("prompt", "Confirm PIN"));
    assert_eq!(second.generation, first.generation + 1);
    assert_eq!(second.owned_by_nav, None);
    assert!(!state.can_accept_beforeunload(token, first.generation));
    assert!(!state.can_accept_beforeunload(token, second.generation));
    assert!(state.is_blocking_for_human());

    // Acking the stale generation never clears the newer prompt.
    state.ack_beforeunload_closed(first.generation);
    assert_eq!(state.active_blocking(), Some(&second.dialog));
}

#[test]
fn page_raised_alert_confirm_and_prompt_always_block_even_during_navigation() {
    for kind in ["alert", "confirm", "prompt"] {
        let mut state = DialogState::default();
        let token = state.begin_agent_navigation().unwrap();
        let opened = dialog(kind, "Are you sure?");
        let tracked = state.on_dialog_opening(opened.clone());
        assert_eq!(tracked.owned_by_nav, None);
        assert!(state.is_blocking_for_human());
        assert!(!state.can_accept_beforeunload(token, tracked.generation));
        assert_eq!(state.active_blocking(), Some(&opened));
        state.end_agent_navigation(token);

        assert_eq!(state.begin_agent_navigation(), Err(opened));
        let error = state.check_page_verb().unwrap_err();
        assert!(error.contains(kind), "{error}");
        assert!(error.contains("Take control"), "{error}");

        state.on_dialog_closed();
        assert!(!state.is_blocking_for_human());
        assert!(state.check_page_verb().is_ok());
    }
}

#[test]
fn pre_existing_beforeunload_blocks_all_agent_verbs_including_navigation() {
    let mut state = DialogState::default();
    let opened = dialog("beforeunload", "Owner draft");
    let tracked = state.on_dialog_opening(opened.clone());
    assert_eq!(tracked.owned_by_nav, None);
    assert!(state.is_blocking_for_human());
    let err = state.check_page_verb().unwrap_err();
    assert!(err.contains("beforeunload"), "{err}");
    assert!(err.contains("Take control"), "{err}");
    assert_eq!(state.begin_agent_navigation(), Err(opened));
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
    BeforeUnloadThenPrompt,
    StallBeforeUnloadAck,
    StallFirstNetworkEnable,
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
    dialog_reply_started: tokio::sync::watch::Sender<bool>,
    network_enable_started: tokio::sync::watch::Sender<bool>,
    network_enable_calls: std::sync::atomic::AtomicUsize,
}

impl NavFakeConnection {
    fn new(behavior: NavDialogBehavior) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            behavior,
            closed: tokio::sync::watch::channel(None).0,
            subscribers: parking_lot::Mutex::new(Vec::new()),
            requests: parking_lot::Mutex::new(Vec::new()),
            beforeunload_accepted: tokio::sync::watch::channel(false).0,
            dialog_reply_started: tokio::sync::watch::channel(false).0,
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
            "Network.enable" => {
                let call = self
                    .network_enable_calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if matches!(self.behavior, NavDialogBehavior::StallFirstNetworkEnable) && call == 0
                {
                    self.network_enable_started.send_replace(true);
                    std::future::pending::<()>().await;
                }
                Ok(json!({}))
            }
            "Page.handleJavaScriptDialog" => {
                if matches!(self.behavior, NavDialogBehavior::StallBeforeUnloadAck) {
                    self.dialog_reply_started.send_replace(true);
                    std::future::pending::<()>().await;
                }
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
                    NavDialogBehavior::StallBeforeUnloadAck => {
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "beforeunload", "message": "Unsaved draft", "url": "https://dirty.test/" }),
                        );
                        std::future::pending::<()>().await;
                        unreachable!()
                    }
                    NavDialogBehavior::BeforeUnloadThenPrompt => {
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "beforeunload", "message": "Leave?", "url": "https://dirty.test/" }),
                        );
                        self.emit(
                            "Page.javascriptDialogClosed",
                            Some("ST1"),
                            json!({ "result": false, "userInput": "" }),
                        );
                        self.emit(
                            "Page.javascriptDialogOpening",
                            Some("ST1"),
                            json!({ "type": "prompt", "message": "Enter code", "url": url }),
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
                    NavDialogBehavior::Clean | NavDialogBehavior::StallFirstNetworkEnable => {
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
    assert!(follow_up.contains("blocked:"), "{follow_up}");
    assert_eq!(conn.count("Page.navigate"), nav_before);
}

#[tokio::test]
async fn agent_goto_refuses_pre_existing_beforeunload_without_answering_it() {
    let (_root, runtime, page, conn) = runtime_with_fake_page(NavDialogBehavior::Clean).await;
    let mut dialogs = runtime.shared.subscribe_dialogs();
    conn.emit(
        "Page.javascriptDialogOpening",
        Some("ST1"),
        json!({ "type": "beforeunload", "message": "Owner draft", "url": "https://owner.test/" }),
    );
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        dialogs.wait_for(Option::is_some),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    assert!(runtime
        .status_text()
        .contains("dialog: beforeunload — Owner draft"));

    let error = runtime
        .navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/refused"),
        )
        .await
        .unwrap_err();
    assert!(error.contains("blocked:"), "{error}");
    assert!(error.contains("beforeunload"), "{error}");
    assert!(error.contains("Owner draft"), "{error}");
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 0);
    assert_eq!(conn.count("Page.navigate"), 0);
}

#[tokio::test]
async fn agent_goto_never_answers_replacement_prompt_after_closed_beforeunload() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(NavDialogBehavior::BeforeUnloadThenPrompt).await;
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/prompt"),
        ),
    )
    .await
    .expect("replacement prompt must surface promptly")
    .unwrap_err();
    assert!(error.contains("prompt"), "{error}");
    assert!(error.contains("Enter code"), "{error}");
    assert_eq!(
        conn.count("Page.handleJavaScriptDialog"),
        0,
        "stale beforeunload must never answer replacement prompt"
    );
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    assert!(runtime
        .status_text()
        .contains("dialog: prompt — Enter code"));
}

#[tokio::test]
async fn cancelling_navigation_before_beforeunload_ack_leaves_actionable_blocking_dialog() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(NavDialogBehavior::StallBeforeUnloadAck).await;
    let mut reply_started = conn.dialog_reply_started.subscribe();
    {
        let nav = runtime.navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/cancel"),
        );
        tokio::pin!(nav);
        tokio::select! {
            _ = &mut nav => panic!("navigation should be stalled on handleJavaScriptDialog"),
            ready = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                reply_started.wait_for(|started| *started),
            ) => {
                ready.expect("handleJavaScriptDialog should be sent").unwrap();
            }
        }
    }
    // Dropping `nav` before the ack must immediately expose the still-open modal.
    assert_eq!(runtime.shared.state().needs_human, "dialog");
    assert!(runtime
        .status_text()
        .contains("dialog: beforeunload — Unsaved draft"));
    let follow_up = runtime
        .navigate(
            &page,
            super::super::input::NavAction::Goto,
            Some("https://next.test/after-cancel"),
        )
        .await
        .unwrap_err();
    assert!(follow_up.contains("blocked:"), "{follow_up}");
    assert_eq!(conn.count("Page.handleJavaScriptDialog"), 1);
}

#[tokio::test]
async fn cancelling_domain_setup_before_network_enable_allows_clean_reentry() {
    let (_root, runtime, page, conn) =
        runtime_with_fake_page(NavDialogBehavior::StallFirstNetworkEnable).await;
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
