use super::*;

const NOTIFICATION_CHAT: i64 = -100186000001;
const NOTIFICATION_TAB: &str = "tab_worker";

#[derive(Default)]
struct FailingNotificationTransport {
    sends: PLMutex<Vec<(Option<i64>, String, bool)>>,
}

#[async_trait::async_trait]
impl crate::bridge::threads::TopicTransport for FailingNotificationTransport {
    async fn create_topic(
        &self,
        _chat_id: i64,
        _name: &str,
    ) -> Result<i64, crate::bridge::threads::TopicFailure> {
        Ok(321)
    }

    async fn probe_topic(
        &self,
        _chat_id: i64,
        _thread_id: i64,
        _name: &str,
    ) -> crate::bridge::threads::TopicProbe {
        crate::bridge::threads::TopicProbe::Open
    }

    async fn delete_topic(
        &self,
        _chat_id: i64,
        _thread_id: i64,
    ) -> Result<(), crate::bridge::threads::TopicFailure> {
        Ok(())
    }

    async fn rename_topic(
        &self,
        _chat_id: i64,
        _thread_id: i64,
        _name: &str,
    ) -> Result<(), crate::bridge::threads::TopicFailure> {
        Ok(())
    }

    async fn send_html(
        &self,
        _chat_id: i64,
        thread_id: Option<i64>,
        html: &str,
        buttons: Option<&InlineKeyboard>,
    ) -> Result<i64, crate::bridge::threads::TopicFailure> {
        self.sends
            .lock()
            .push((thread_id, html.to_string(), buttons.is_some()));
        Err(match thread_id {
            Some(_) => crate::bridge::threads::TopicFailure::ThreadGone,
            None => {
                crate::bridge::threads::TopicFailure::Rejected("main chat unavailable".to_string())
            }
        })
    }

    async fn edit_html(&self, _chat_id: i64, _message_id: i64, _html: &str) -> bool {
        false
    }
}

struct NotificationTabs;

impl crate::bridge::threads::ThreadTabs for NotificationTabs {
    fn sender_identity(&self, tab_id: &str) -> Option<crate::bridge::threads::SenderIdentity> {
        (tab_id == NOTIFICATION_TAB).then(|| crate::bridge::threads::SenderIdentity {
            tab_id: tab_id.to_string(),
            title: "Worker".to_string(),
            role: crate::bridge::threads::SenderRole::Worker,
            workspace: "codeterm".to_string(),
            machine: "test".to_string(),
        })
    }

    fn live_tab_titles(&self) -> Vec<(String, String)> {
        vec![(NOTIFICATION_TAB.to_string(), "Worker".to_string())]
    }

    fn is_live_tab(&self, tab_id: &str) -> bool {
        tab_id == NOTIFICATION_TAB
    }

    fn is_visible_tab(&self, tab_id: &str) -> bool {
        self.is_live_tab(tab_id)
    }

    fn is_general_tab(&self, _tab_id: &str) -> bool {
        false
    }
}

struct NotificationBridgeTab {
    threads: Arc<crate::bridge::threads::ThreadService>,
    tabs: NotificationTabs,
    placements: PLMutex<Vec<(PlacementReceipt, String)>>,
}

impl crate::ports::BridgeTabPort for NotificationBridgeTab {
    fn resolve_general_tab(&self) -> Result<String, String> {
        unreachable!()
    }

    fn tab_agent_type(&self, _tab_id: &str) -> Option<String> {
        None
    }

    fn transcribe_registry(&self) -> Arc<crate::transcribe::TranscribeRegistry> {
        Arc::new(crate::transcribe::TranscribeRegistry::new())
    }

    fn bridge_outbound(&self) -> OutboundSender {
        create_outbound_channel().0
    }

    fn voice_transcribe_config(&self) -> codeterm_types::config::schema::VoiceTranscribeConfig {
        Default::default()
    }

    fn text_hooks_config(&self) -> codeterm_types::config::schema::TextHooksConfig {
        Default::default()
    }

    fn deliver_inbound(
        &self,
        _tab_id: &str,
        _prefixed: &str,
        _agent_type: Option<&str>,
    ) -> Result<(), String> {
        unreachable!()
    }

    fn queue_inbound(&self, _tab_id: &str, _prefixed: String) {
        unreachable!()
    }

    fn claim_channel_answer(&self, _callback_data: &str) -> bool {
        false
    }

    fn record_channel_placement(&self, receipt: &PlacementReceipt, locator: &str) {
        self.placements
            .lock()
            .push((receipt.clone(), locator.to_string()));
    }

    fn telegram_token(&self) -> Option<String> {
        None
    }

    fn telegram_token_path(&self) -> std::path::PathBuf {
        std::path::PathBuf::new()
    }

    fn threads(&self) -> Arc<crate::bridge::threads::ThreadService> {
        self.threads.clone()
    }

    fn thread_tabs(&self) -> &dyn crate::bridge::threads::ThreadTabs {
        &self.tabs
    }
}

#[tokio::test]
async fn failed_notification_does_not_fall_through_to_raw_chat_send() {
    let transport = Arc::new(FailingNotificationTransport::default());
    let threads = Arc::new(crate::bridge::threads::ThreadService::new(
        crate::bridge::threads::ThreadRegistry::in_memory(),
    ));
    threads.attach(transport.clone(), 77_700);
    let tab = Arc::new(NotificationBridgeTab {
        threads,
        tabs: NotificationTabs,
        placements: PLMutex::new(Vec::new()),
    });
    let bridge_tab: Arc<dyn crate::ports::BridgeTabPort> = tab.clone();
    let captured = Arc::new(PLMutex::new(Vec::new()));
    let api_base = spawn_fake_telegram_with_methods(captured.clone()).await;
    let receipt = PlacementReceipt {
        service: "telegram".to_string(),
        tag: "choice".to_string(),
    };
    let message = OutboundMessage {
        user_id: Some(NOTIFICATION_CHAT),
        content: OutboundContent::Text {
            text: "Permission request".to_string(),
        },
        append: false,
        buttons: Some(vec![vec![crate::bridge::traits::InlineButton {
            text: "Allow".to_string(),
            data: Some("choice:allow".to_string()),
            url: None,
        }]]),
        edit: None,
        receipt: Some(receipt),
        route: OutboundRoute::TabNotification {
            tab_id: NOTIFICATION_TAB.to_string(),
        },
    };

    threaded::send_outbound(
        &bridge_tab,
        &reqwest::Client::new(),
        &api_base,
        "FAKETOKEN",
        NOTIFICATION_CHAT,
        &message,
        &mut std::collections::HashMap::new(),
    )
    .await;

    let sends = transport.sends.lock();
    assert_eq!(sends.len(), 2, "thread service attempts: {sends:?}");
    assert_eq!(sends[0].0, Some(321));
    assert!(sends[0].1.contains("Permission request"));
    assert!(sends[0].2, "the topic attempt carries the button");
    assert_eq!(sends[1].0, None, "one main-chat fallback is attempted");
    assert!(sends[1].1.contains("Permission request"));
    assert!(sends[1].2, "the fallback carries the button");
    assert!(
        captured.lock().is_empty(),
        "a failed thread fallback must not reach the generic raw-chat route"
    );
    assert!(tab.placements.lock().is_empty());
    let stored: serde_json::Value = serde_json::from_str(&tab.threads.stored_json()).unwrap();
    assert_eq!(stored["bindings"][0]["stale"], true);
}

#[tokio::test]
async fn tab_notification_edits_use_the_regular_rewrite_route() {
    let transport = Arc::new(FailingNotificationTransport::default());
    let threads = Arc::new(crate::bridge::threads::ThreadService::new(
        crate::bridge::threads::ThreadRegistry::in_memory(),
    ));
    threads.attach(transport.clone(), 77_700);
    let tab = Arc::new(NotificationBridgeTab {
        threads,
        tabs: NotificationTabs,
        placements: PLMutex::new(Vec::new()),
    });
    let bridge_tab: Arc<dyn crate::ports::BridgeTabPort> = tab.clone();
    let captured = Arc::new(PLMutex::new(Vec::new()));
    let api_base = spawn_fake_telegram_with_methods(captured.clone()).await;
    let message = OutboundMessage {
        user_id: Some(NOTIFICATION_CHAT),
        content: OutboundContent::Text {
            text: "Updated permission request".to_string(),
        },
        append: false,
        buttons: None,
        edit: Some(super::super::OutboundEdit { message_id: 55 }),
        receipt: None,
        route: OutboundRoute::TabNotification {
            tab_id: NOTIFICATION_TAB.to_string(),
        },
    };

    threaded::send_outbound(
        &bridge_tab,
        &reqwest::Client::new(),
        &api_base,
        "FAKETOKEN",
        NOTIFICATION_CHAT,
        &message,
        &mut std::collections::HashMap::new(),
    )
    .await;

    let requests = captured.lock();
    assert_eq!(requests.len(), 1, "rewrite request: {requests:?}");
    assert!(requests[0].0.ends_with("/editMessageText"));
    assert_eq!(requests[0].1["message_id"].as_i64(), Some(55));
    assert_eq!(
        requests[0].1["text"].as_str(),
        Some("Updated permission request")
    );
    assert!(transport.sends.lock().is_empty());
}
