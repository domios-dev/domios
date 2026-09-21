use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use codeterm_bridge::bridge::manager::{
    threaded, OutboundMessage, OutboundRoute, OutboundSender, PlacementReceipt,
};
use codeterm_bridge::bridge::threads::identity::SenderIdentity;
use codeterm_bridge::bridge::threads::transport::{TopicFailure, TopicProbe, TopicTransport};
use codeterm_bridge::bridge::threads::{
    SenderRole, TabText, ThreadRegistry, ThreadService, ThreadTabs, TopicsState,
    TOPICS_DISABLED_NOTICE,
};
use codeterm_bridge::bridge::traits::{InlineButton, InlineKeyboard, OutboundContent};
use codeterm_bridge::ports::BridgeTabPort;
use codeterm_bridge::TranscribeRegistry;
use codeterm_types::config::schema::{TextHooksConfig, VoiceTranscribeConfig};
use parking_lot::Mutex;

const CHAT: i64 = 186_000_001;
const GENERAL: &str = "tab_general";
const WORKER: &str = "tab_worker";
const HIDDEN: &str = "tab_hidden";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Create {
        name: String,
        thread: i64,
    },
    Send {
        thread: Option<i64>,
        html: String,
        buttons: bool,
        message_id: Option<i64>,
    },
}

#[derive(Default)]
struct RecordingTransport {
    calls: Mutex<Vec<Call>>,
    topics_disabled: bool,
    fail_next_threaded_send: Mutex<bool>,
    fail_next_main_chat_send: Mutex<bool>,
    next_id: Mutex<i64>,
}

impl RecordingTransport {
    fn enabled() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn topics_disabled() -> Arc<Self> {
        Arc::new(Self {
            topics_disabled: true,
            ..Self::default()
        })
    }

    fn fail_next_threaded_send(&self) {
        *self.fail_next_threaded_send.lock() = true;
    }

    fn fail_next_main_chat_send(&self) {
        *self.fail_next_main_chat_send.lock() = true;
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().clone()
    }

    fn next(&self) -> i64 {
        let mut next = self.next_id.lock();
        *next += 1;
        1000 + *next
    }
}

#[async_trait]
impl TopicTransport for RecordingTransport {
    async fn create_topic(&self, _chat_id: i64, name: &str) -> Result<i64, TopicFailure> {
        let thread = self.next();
        self.calls.lock().push(Call::Create {
            name: name.to_string(),
            thread,
        });
        if self.topics_disabled {
            Err(TopicFailure::TopicsUnavailable)
        } else {
            Ok(thread)
        }
    }

    async fn probe_topic(&self, _chat_id: i64, _thread_id: i64, _name: &str) -> TopicProbe {
        TopicProbe::Open
    }

    async fn delete_topic(&self, _chat_id: i64, _thread_id: i64) -> Result<(), TopicFailure> {
        Ok(())
    }

    async fn rename_topic(
        &self,
        _chat_id: i64,
        _thread_id: i64,
        _name: &str,
    ) -> Result<(), TopicFailure> {
        Ok(())
    }

    async fn send_html(
        &self,
        _chat_id: i64,
        thread: Option<i64>,
        html: &str,
        buttons: Option<&InlineKeyboard>,
    ) -> Result<i64, TopicFailure> {
        let fail = if thread.is_some() {
            std::mem::take(&mut *self.fail_next_threaded_send.lock())
        } else {
            std::mem::take(&mut *self.fail_next_main_chat_send.lock())
        };
        let message_id = (!fail).then(|| self.next());
        self.calls.lock().push(Call::Send {
            thread,
            html: html.to_string(),
            buttons: buttons.is_some(),
            message_id,
        });
        match message_id {
            Some(message_id) => Ok(message_id),
            None if thread.is_some() => Err(TopicFailure::ThreadGone),
            None => Err(TopicFailure::Rejected("main chat send failed".to_string())),
        }
    }

    async fn edit_html(&self, _chat_id: i64, _message_id: i64, _html: &str) -> bool {
        true
    }
}

#[derive(Default)]
struct Tabs {
    hidden: Mutex<Vec<String>>,
}

impl ThreadTabs for Tabs {
    fn sender_identity(&self, tab_id: &str) -> Option<SenderIdentity> {
        let (title, role) = match tab_id {
            GENERAL => ("General", SenderRole::General),
            WORKER => ("Worker", SenderRole::Worker),
            HIDDEN => ("Hidden", SenderRole::Worker),
            _ => return None,
        };
        Some(SenderIdentity {
            tab_id: tab_id.to_string(),
            title: title.to_string(),
            role,
            workspace: "codeterm".to_string(),
            machine: "BLACKSTAR".to_string(),
        })
    }

    fn live_tab_titles(&self) -> Vec<(String, String)> {
        vec![
            (GENERAL.to_string(), "General".to_string()),
            (WORKER.to_string(), "Worker".to_string()),
            (HIDDEN.to_string(), "Hidden".to_string()),
        ]
    }

    fn is_live_tab(&self, tab_id: &str) -> bool {
        self.sender_identity(tab_id).is_some()
    }

    fn is_visible_tab(&self, tab_id: &str) -> bool {
        self.is_live_tab(tab_id) && !self.hidden.lock().iter().any(|hidden| hidden == tab_id)
    }

    fn is_general_tab(&self, tab_id: &str) -> bool {
        tab_id == GENERAL
    }
}

struct TestTab {
    threads: Arc<ThreadService>,
    tabs: Tabs,
    placements: Mutex<Vec<(PlacementReceipt, String)>>,
}

impl BridgeTabPort for TestTab {
    fn resolve_general_tab(&self) -> Result<String, String> {
        unreachable!()
    }

    fn tab_agent_type(&self, _tab_id: &str) -> Option<String> {
        None
    }

    fn transcribe_registry(&self) -> Arc<TranscribeRegistry> {
        unreachable!()
    }

    fn bridge_outbound(&self) -> OutboundSender {
        unreachable!()
    }

    fn voice_transcribe_config(&self) -> VoiceTranscribeConfig {
        unreachable!()
    }

    fn text_hooks_config(&self) -> TextHooksConfig {
        unreachable!()
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

    fn telegram_token_path(&self) -> PathBuf {
        PathBuf::new()
    }

    fn threads(&self) -> Arc<ThreadService> {
        self.threads.clone()
    }

    fn thread_tabs(&self) -> &dyn ThreadTabs {
        &self.tabs
    }
}

fn test_tab(transport: Arc<RecordingTransport>) -> Arc<TestTab> {
    let threads = Arc::new(ThreadService::new(ThreadRegistry::in_memory()));
    threads.attach(transport, 777_000);
    Arc::new(TestTab {
        threads,
        tabs: Tabs::default(),
        placements: Mutex::new(Vec::new()),
    })
}

fn notification(tab_id: &str, markdown: &str, buttons: Option<InlineKeyboard>) -> OutboundMessage {
    OutboundMessage {
        user_id: Some(CHAT),
        content: OutboundContent::Text {
            text: markdown.to_string(),
        },
        append: false,
        buttons,
        edit: None,
        receipt: Some(PlacementReceipt {
            service: "telegram".to_string(),
            tag: "record-1".to_string(),
        }),
        route: OutboundRoute::TabNotification {
            tab_id: tab_id.to_string(),
        },
    }
}

fn text(tab_id: &str, markdown: &str, buttons: Option<InlineKeyboard>) -> TabText {
    TabText {
        tab_id: tab_id.to_string(),
        chat_id: CHAT,
        markdown: markdown.to_string(),
        append: false,
        buttons,
    }
}

fn keyboard() -> InlineKeyboard {
    vec![vec![InlineButton {
        text: "Ship it".to_string(),
        data: Some("na:record-1:0".to_string()),
        url: None,
    }]]
}

fn delivered_sends<'a>(calls: &'a [Call], text: &str) -> Vec<&'a Call> {
    calls
        .iter()
        .filter(|call| {
            matches!(call, Call::Send { html, message_id: Some(_), .. } if html.contains(text) && html != TOPICS_DISABLED_NOTICE)
        })
        .collect()
}

#[tokio::test]
async fn first_notification_creates_topic_before_sending_full_text_and_buttons_inside_it() {
    let transport = RecordingTransport::enabled();
    let tab = test_tab(transport.clone());
    let port: Arc<dyn BridgeTabPort> = tab.clone();

    assert!(
        threaded::send_notification(
            &port,
            CHAT,
            &notification(WORKER, "Ship **the whole change**?", Some(keyboard())),
        )
        .await
    );

    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    let Call::Create { name, thread } = &calls[0] else {
        panic!("thread creation must be the first transport call: {calls:?}");
    };
    assert_eq!(name, "Worker");
    let Call::Send {
        thread: Some(sent_thread),
        html,
        buttons,
        message_id: Some(message_id),
    } = &calls[1]
    else {
        panic!("the only notification send must carry the created thread: {calls:?}");
    };
    assert_eq!(sent_thread, thread);
    assert!(html.contains("Ship <strong>the whole change</strong>?"));
    assert!(*buttons);
    assert_eq!(
        tab.placements.lock().as_slice(),
        &[(
            receipt(),
            codeterm_bridge::bridge::telegram::locator(CHAT, *message_id)
        )]
    );
}

#[tokio::test]
async fn later_notification_reuses_the_binding_with_one_thread_send() {
    let transport = RecordingTransport::enabled();
    let tab = test_tab(transport.clone());
    let port: Arc<dyn BridgeTabPort> = tab.clone();

    assert!(threaded::send_notification(&port, CHAT, &notification(WORKER, "first", None),).await);
    let first_thread = match transport.calls().first().cloned().expect("first call") {
        Call::Create { thread, .. } => thread,
        call => panic!("expected create first, found {call:?}"),
    };
    let offset = transport.calls().len();

    assert!(threaded::send_notification(&port, CHAT, &notification(WORKER, "second", None),).await);

    let later = &transport.calls()[offset..];
    assert_eq!(later.len(), 1);
    assert!(
        matches!(&later[0], Call::Send { thread: Some(thread), message_id: Some(_), .. } if *thread == first_thread)
    );
}

#[tokio::test]
async fn general_and_hidden_notifications_send_once_in_main_chat_without_creating_topics() {
    for (tab_id, hidden) in [(GENERAL, false), (HIDDEN, true)] {
        let transport = RecordingTransport::enabled();
        let tab = test_tab(transport.clone());
        if hidden {
            tab.tabs.hidden.lock().push(tab_id.to_string());
        }
        let port: Arc<dyn BridgeTabPort> = tab.clone();

        assert!(
            threaded::send_notification(&port, CHAT, &notification(tab_id, "hello", None),).await
        );

        let calls = transport.calls();
        assert!(calls
            .iter()
            .all(|call| !matches!(call, Call::Create { .. })));
        let sends = delivered_sends(&calls, "hello");
        assert_eq!(sends.len(), 1);
        assert!(matches!(sends[0], Call::Send { thread: None, .. }));
    }
}

#[tokio::test]
async fn disabled_topics_send_the_notification_once_in_main_chat() {
    let transport = RecordingTransport::topics_disabled();
    let tab = test_tab(transport.clone());
    let port: Arc<dyn BridgeTabPort> = tab.clone();

    assert!(threaded::send_notification(&port, CHAT, &notification(WORKER, "hello", None),).await);

    let calls = transport.calls();
    let sends = delivered_sends(&calls, "hello");
    assert_eq!(sends.len(), 1);
    assert!(matches!(sends[0], Call::Send { thread: None, .. }));
    assert_eq!(
        tab.threads.menu_state(WORKER, Some(CHAT)).await.topics,
        TopicsState::Unavailable
    );
}

#[tokio::test]
async fn notification_to_a_deleted_topic_falls_back_to_main_chat_once() {
    let transport = RecordingTransport::enabled();
    let tab = test_tab(transport.clone());
    assert!(tab
        .threads
        .notification_thread_id(&tab.tabs, WORKER, CHAT)
        .await
        .is_some());
    transport.fail_next_threaded_send();
    let port: Arc<dyn BridgeTabPort> = tab.clone();

    assert!(
        threaded::send_notification(
            &port,
            CHAT,
            &notification(
                WORKER,
                "Please choose **Ship this whole change**?",
                Some(keyboard()),
            ),
        )
        .await
    );

    let calls = transport.calls();
    assert_eq!(calls.len(), 3);
    assert!(matches!(
        &calls[1],
        Call::Send {
            thread: Some(_),
            message_id: None,
            buttons: true,
            ..
        }
    ));
    let Call::Send {
        thread: None,
        html,
        buttons: true,
        message_id: Some(_),
    } = &calls[2]
    else {
        panic!("fallback must deliver the full notification with its buttons: {calls:?}");
    };
    let header = tab
        .tabs
        .sender_identity(WORKER)
        .expect("worker identity")
        .header();
    assert!(html.contains(&header));
    assert!(html.contains("Please choose <strong>Ship this whole change</strong>?"));
    assert_eq!(delivered_sends(&calls, "Ship this whole change").len(), 1);
}

#[tokio::test]
async fn choice_receipt_follows_the_fallback_message() {
    let transport = RecordingTransport::enabled();
    let tab = test_tab(transport.clone());
    assert!(tab
        .threads
        .notification_thread_id(&tab.tabs, WORKER, CHAT)
        .await
        .is_some());
    transport.fail_next_threaded_send();
    let port: Arc<dyn BridgeTabPort> = tab.clone();
    let message = notification(WORKER, "Deploy?", Some(keyboard()));

    assert!(threaded::send_notification(&port, CHAT, &message).await);

    let calls = transport.calls();
    let Call::Send {
        thread: None,
        buttons: true,
        message_id: Some(message_id),
        ..
    } = calls.last().expect("fallback message")
    else {
        panic!("the fallback message must carry the choice buttons: {calls:?}");
    };
    assert_eq!(
        tab.placements.lock().as_slice(),
        &[(
            message.receipt.clone().expect("receipt"),
            codeterm_bridge::bridge::telegram::locator(CHAT, *message_id)
        )]
    );
}

#[tokio::test]
async fn failed_notification_placement_is_not_claimed_as_handled() {
    let transport = RecordingTransport::enabled();
    let tab = test_tab(transport.clone());
    assert!(tab
        .threads
        .notification_thread_id(&tab.tabs, WORKER, CHAT)
        .await
        .is_some());
    transport.fail_next_threaded_send();
    transport.fail_next_main_chat_send();
    let port: Arc<dyn BridgeTabPort> = tab.clone();

    assert!(
        !threaded::send_notification(
            &port,
            CHAT,
            &notification(WORKER, "Deploy?", Some(keyboard())),
        )
        .await
    );

    let calls = transport.calls();
    assert!(matches!(
        calls.last(),
        Some(Call::Send {
            thread: None,
            message_id: None,
            ..
        })
    ));
    assert!(delivered_sends(&calls, "Deploy?").is_empty());
    assert!(tab.placements.lock().is_empty());
}

#[tokio::test]
async fn route_text_and_notifications_share_the_thread_fallback() {
    let notification_transport = RecordingTransport::enabled();
    let notification_tab = test_tab(notification_transport.clone());
    assert!(notification_tab
        .threads
        .notification_thread_id(&notification_tab.tabs, WORKER, CHAT)
        .await
        .is_some());
    notification_transport.fail_next_threaded_send();
    let port: Arc<dyn BridgeTabPort> = notification_tab.clone();

    assert!(
        threaded::send_notification(
            &port,
            CHAT,
            &notification(WORKER, "notification", Some(keyboard())),
        )
        .await
    );
    assert!(matches!(
        notification_transport.calls().last(),
        Some(Call::Send {
            thread: None,
            buttons: true,
            message_id: Some(_),
            ..
        })
    ));

    let route_transport = RecordingTransport::enabled();
    let route_tab = test_tab(route_transport.clone());
    assert!(route_tab
        .threads
        .notification_thread_id(&route_tab.tabs, WORKER, CHAT)
        .await
        .is_some());
    route_transport.fail_next_threaded_send();

    let placed = route_tab
        .threads
        .route_text(&route_tab.tabs, text(WORKER, "route", Some(keyboard())))
        .await
        .expect("route fallback placement");

    assert!(matches!(
        route_transport.calls().last(),
        Some(Call::Send { thread: None, buttons: true, message_id: Some(message_id), .. })
            if *message_id == placed
    ));
}

fn receipt() -> PlacementReceipt {
    PlacementReceipt {
        service: "telegram".to_string(),
        tag: "record-1".to_string(),
    }
}
