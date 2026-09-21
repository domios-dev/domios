use std::sync::Arc;

use async_trait::async_trait;
use codeterm_bridge::bridge::telegram::locator;
use codeterm_bridge::bridge::threads::identity::SenderIdentity;
use codeterm_bridge::bridge::threads::transport::{TopicFailure, TopicProbe, TopicTransport};
use codeterm_bridge::bridge::threads::{
    SenderRole, TabText, ThreadRegistry, ThreadService, ThreadTabs, TopicsState,
    TOPICS_DISABLED_NOTICE,
};
use codeterm_bridge::bridge::traits::{InlineButton, InlineKeyboard};
use parking_lot::Mutex;

const CHAT: i64 = 186_000_001;
const GENERAL: &str = "tab_general";
const WORKER: &str = "tab_worker";
const HIDDEN: &str = "tab_hidden";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Create { name: String, thread: i64 },
    Send {
        thread: Option<i64>,
        html: String,
        buttons: bool,
        message: i64,
    },
}

#[derive(Default)]
struct RecordingTransport {
    calls: Mutex<Vec<Call>>,
    topics_disabled: bool,
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
        let message = self.next();
        self.calls.lock().push(Call::Send {
            thread,
            html: html.to_string(),
            buttons: buttons.is_some(),
            message,
        });
        Ok(message)
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

fn attached(transport: Arc<RecordingTransport>) -> ThreadService {
    let threads = ThreadService::new(ThreadRegistry::in_memory());
    threads.attach(transport, 777_000);
    threads
}

fn text(tab_id: &str, markdown: &str) -> TabText {
    TabText {
        tab_id: tab_id.to_string(),
        chat_id: CHAT,
        markdown: markdown.to_string(),
        append: false,
        buttons: None,
    }
}

fn notify_send(calls: &[Call], html: &str) -> Vec<&Call> {
    calls.iter().filter(|call| {
        matches!(call, Call::Send { html: body, .. } if body.contains(html) && body != TOPICS_DISABLED_NOTICE)
    })
        .collect()
}

async fn notify(
    threads: &ThreadService,
    tabs: &Tabs,
    transport: &RecordingTransport,
    text: TabText,
) -> Option<i64> {
    match threads
        .notification_thread_id(tabs, &text.tab_id, text.chat_id)
        .await
    {
        Some(thread_id) => {
            threads
                .send_notification_to_thread(tabs, text, thread_id)
                .await
        }
        None => transport
            .send_html(CHAT, None, &text.markdown, text.buttons.as_ref())
            .await
            .ok(),
    }
}

#[tokio::test]
async fn first_notification_creates_topic_before_sending_full_text_and_buttons_inside_it() {
    let transport = RecordingTransport::enabled();
    let threads = attached(transport.clone());
    let tabs = Tabs::default();
    let keyboard = vec![vec![InlineButton {
        text: "Ship it".to_string(),
        data: Some("na:record-1:0".to_string()),
        url: None,
    }]];

    let placed = notify(
        &threads,
        &tabs,
        &transport,
        TabText {
            buttons: Some(keyboard),
            ..text(WORKER, "Ship **the whole change**?")
        },
    )
        .await
        .expect("notification sent");

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
        message,
    } = &calls[1]
    else {
        panic!("the only notify send must carry the created thread: {calls:?}");
    };
    assert_eq!(sent_thread, thread);
    assert_eq!(message, &placed);
    assert!(html.contains("Ship <strong>the whole change</strong>?"));
    assert!(*buttons);
}

#[tokio::test]
async fn later_notification_reuses_the_binding_with_one_thread_send() {
    let transport = RecordingTransport::enabled();
    let threads = attached(transport.clone());
    let tabs = Tabs::default();

    notify(&threads, &tabs, &transport, text(WORKER, "first"))
        .await
        .expect("first notification sent");
    let first_thread = match transport.calls().first().cloned().expect("first call") {
        Call::Create { thread, .. } => thread,
        ref call => panic!("expected create first, found {call:?}"),
    };
    let offset = transport.calls().len();

    notify(&threads, &tabs, &transport, text(WORKER, "second"))
        .await
        .expect("second notification sent");

    let later = &transport.calls()[offset..];
    assert_eq!(later.len(), 1);
    assert!(matches!(&later[0], Call::Send { thread: Some(thread), .. } if *thread == first_thread));
}

#[tokio::test]
async fn general_and_hidden_notifications_send_once_in_main_chat_without_creating_topics() {
    for (tab_id, hidden) in [(GENERAL, false), (HIDDEN, true)] {
        let transport = RecordingTransport::enabled();
        let threads = attached(transport.clone());
        let tabs = Tabs::default();
        if hidden {
            tabs.hidden.lock().push(tab_id.to_string());
        }

        notify(&threads, &tabs, &transport, text(tab_id, "hello"))
            .await
            .expect("notification sent");

        let calls = transport.calls();
        assert!(calls.iter().all(|call| !matches!(call, Call::Create { .. })));
        let sends = notify_send(&calls, "hello");
        assert_eq!(sends.len(), 1);
        assert!(matches!(sends[0], Call::Send { thread: None, .. }));
    }
}

#[tokio::test]
async fn disabled_topics_send_the_notification_once_in_main_chat() {
    let transport = RecordingTransport::topics_disabled();
    let threads = attached(transport.clone());

    notify(&threads, &Tabs::default(), &transport, text(WORKER, "hello"))
        .await
        .expect("fallback notification sent");

    let calls = transport.calls();
    let sends = notify_send(&calls, "hello");
    assert_eq!(sends.len(), 1);
    assert!(matches!(sends[0], Call::Send { thread: None, .. }));
    assert_eq!(
        threads.menu_state(WORKER, Some(CHAT)).await.topics,
        TopicsState::Unavailable
    );
}

#[tokio::test]
async fn choice_receipt_locator_addresses_the_in_thread_notification_message() {
    let transport = RecordingTransport::enabled();
    let threads = attached(transport.clone());
    let placed = notify(
        &threads,
        &Tabs::default(),
        &transport,
        TabText {
            buttons: Some(vec![vec![InlineButton {
                text: "Ship it".to_string(),
                data: Some("na:record-1:0".to_string()),
                url: None,
            }]]),
            ..text(WORKER, "Deploy?")
        },
    )
    .await
    .expect("notification sent");
    let calls = transport.calls();
    let Call::Send {
        thread: Some(_),
        buttons,
        message,
        ..
    } = calls[1].clone()
    else {
        panic!("notification was not sent into its topic");
    };

    assert!(buttons);
    assert_eq!(placed, message);
    assert_eq!(locator(CHAT, placed), format!("{CHAT}:{message}"));
}
