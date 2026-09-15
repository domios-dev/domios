//! Fixtures for per-tab Telegram topics: update parsing, inbound routing,
//! bindings, sender headers, open authority, topic fallback and tab close.

use std::sync::Arc;

use async_trait::async_trait;
use codeterm_bridge::bridge::manager::{OutboundMessage, OutboundRoute};
use codeterm_bridge::bridge::telegram::reply_context;
use codeterm_bridge::bridge::threads::identity::{topic_name, HEADER_SEPARATOR};
use codeterm_bridge::bridge::threads::transport::{
    bot_id_from_token, classify_failure, topic_link,
};
use codeterm_bridge::bridge::threads::{
    CloseOutcome, InboundOrigin, InboundRoute, SenderIdentity, SenderRole, TabText, ThreadCaller,
    ThreadRegistry, ThreadService, ThreadTabs, TopicFailure, TopicTransport, TopicsState,
    TOPICS_DISABLED_NOTICE,
};
use codeterm_bridge::bridge::traits::{InlineButton, InlineKeyboard, ReplyContext};
use codeterm_types::message_markers;
use parking_lot::Mutex;
use serde_json::json;

use crate::notification::channel_ask::{ChannelChoice, PushMessage};
use crate::notification::channels::{Audience, ChannelHost, PushChannel};

mod attach;

const CHAT: i64 = 186_000_001;
const SUPERGROUP: i64 = -1_001_234_567_890;
const TOKEN: &str = "777000:do-not-store-this-secret";
const GENERAL: &str = "tab_general";
const PLANCK: &str = "tab_planck";
const ROWAN: &str = "tab_rowan";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Create { name: String },
    Delete { thread: i64 },
    Rename { thread: i64, name: String },
    Send(Sent),
    Edit { message: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    thread: Option<i64>,
    html: String,
    buttons: bool,
    message: i64,
}

#[derive(Default)]
struct FakeTransport {
    calls: Mutex<Vec<Call>>,
    topics_disabled: bool,
    delete_fails: bool,
    gone_threads: Mutex<Vec<i64>>,
    next_id: Mutex<i64>,
}

impl FakeTransport {
    fn enabled() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn topics_disabled() -> Arc<Self> {
        Arc::new(Self {
            topics_disabled: true,
            ..Self::default()
        })
    }

    fn failing_delete() -> Arc<Self> {
        Arc::new(Self {
            delete_fails: true,
            ..Self::default()
        })
    }

    fn next(&self) -> i64 {
        let mut id = self.next_id.lock();
        *id += 1;
        1000 + *id
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().clone()
    }

    fn creates(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Create { name } => Some(name),
                _ => None,
            })
            .collect()
    }

    fn sends(&self) -> Vec<Sent> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::Send(sent) => Some(sent),
                _ => None,
            })
            .collect()
    }

    fn notices(&self) -> usize {
        self.sends()
            .iter()
            .filter(|sent| sent.html == TOPICS_DISABLED_NOTICE)
            .count()
    }
}

#[async_trait]
impl TopicTransport for FakeTransport {
    async fn create_topic(&self, chat_id: i64, name: &str) -> Result<i64, TopicFailure> {
        assert!(chat_id == CHAT || chat_id == SUPERGROUP);
        tokio::task::yield_now().await;
        self.calls.lock().push(Call::Create {
            name: name.to_string(),
        });
        if self.topics_disabled {
            return Err(TopicFailure::TopicsUnavailable);
        }
        Ok(self.next())
    }

    async fn delete_topic(&self, _chat_id: i64, thread_id: i64) -> Result<(), TopicFailure> {
        self.calls.lock().push(Call::Delete { thread: thread_id });
        if self.delete_fails {
            return Err(TopicFailure::Rejected("not enough rights".to_string()));
        }
        Ok(())
    }

    async fn rename_topic(
        &self,
        _chat_id: i64,
        thread_id: i64,
        name: &str,
    ) -> Result<(), TopicFailure> {
        self.calls.lock().push(Call::Rename {
            thread: thread_id,
            name: name.to_string(),
        });
        Ok(())
    }

    async fn send_html(
        &self,
        _chat_id: i64,
        thread_id: Option<i64>,
        html: &str,
        buttons: Option<&InlineKeyboard>,
    ) -> Result<i64, TopicFailure> {
        tokio::task::yield_now().await;
        if thread_id.is_some_and(|thread| self.gone_threads.lock().contains(&thread)) {
            return Err(TopicFailure::ThreadGone);
        }
        let message = self.next();
        self.calls.lock().push(Call::Send(Sent {
            thread: thread_id,
            html: html.to_string(),
            buttons: buttons.is_some(),
            message,
        }));
        Ok(message)
    }

    async fn edit_html(&self, _chat_id: i64, message_id: i64, _html: &str) -> bool {
        self.calls.lock().push(Call::Edit {
            message: message_id,
        });
        true
    }
}

struct FakeTabs {
    tabs: Mutex<Vec<SenderIdentity>>,
}

impl FakeTabs {
    fn new() -> Self {
        let tabs = Self {
            tabs: Mutex::new(Vec::new()),
        };
        tabs.add(GENERAL, "General", SenderRole::General);
        tabs
    }

    fn add(&self, tab_id: &str, title: &str, role: SenderRole) {
        self.tabs.lock().push(SenderIdentity {
            tab_id: tab_id.to_string(),
            title: title.to_string(),
            role,
            workspace: "codeterm".to_string(),
            machine: "BLACKSTAR".to_string(),
        });
    }

    fn close(&self, tab_id: &str) {
        self.tabs.lock().retain(|tab| tab.tab_id != tab_id);
    }

    fn header(&self, tab_id: &str) -> String {
        self.sender_identity(tab_id).expect("live tab").header()
    }
}

impl ThreadTabs for FakeTabs {
    fn sender_identity(&self, tab_id: &str) -> Option<SenderIdentity> {
        self.tabs
            .lock()
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .cloned()
    }

    fn live_tab_titles(&self) -> Vec<(String, String)> {
        self.tabs
            .lock()
            .iter()
            .map(|tab| (tab.tab_id.clone(), tab.title.clone()))
            .collect()
    }

    fn is_live_tab(&self, tab_id: &str) -> bool {
        self.tabs.lock().iter().any(|tab| tab.tab_id == tab_id)
    }

    fn is_general_tab(&self, tab_id: &str) -> bool {
        tab_id == GENERAL
    }
}

fn attached_with(registry: ThreadRegistry, transport: Arc<FakeTransport>) -> ThreadService {
    let threads = ThreadService::new(registry);
    threads.attach(transport, bot_id_from_token(TOKEN));
    threads
}

fn attached(transport: Arc<FakeTransport>) -> ThreadService {
    attached_with(ThreadRegistry::in_memory(), transport)
}

fn tabs_with_planck() -> FakeTabs {
    let tabs = FakeTabs::new();
    tabs.add(PLANCK, "Planck", SenderRole::Orchestrator);
    tabs
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

fn choice_keyboard() -> InlineKeyboard {
    vec![vec![InlineButton {
        text: "Ship it".to_string(),
        data: Some("na:record-1:0".to_string()),
        url: None,
    }]]
}

fn topic_post(transport: &FakeTransport) -> Sent {
    transport
        .sends()
        .into_iter()
        .find(|sent| sent.thread.is_some())
        .expect("a post inside the topic")
}

fn tab_caller(tab_id: &str) -> ThreadCaller<'_> {
    ThreadCaller::Tab {
        tab_id,
        is_general: tab_id == GENERAL,
    }
}

#[test]
fn telegram_topic_update_in_a_topic_carries_its_thread() {
    let update = json!({
        "message_id": 55,
        "chat": { "id": CHAT },
        "is_topic_message": true,
        "message_thread_id": 42,
        "reply_to_message": { "message_id": 42, "forum_topic_created": { "name": "Planck" } },
        "text": "hi",
    });
    assert_eq!(
        reply_context(&update),
        ReplyContext::Telegram {
            chat_id: CHAT,
            thread_id: Some(42),
            reply_to_message_id: None,
        },
        "the topic-created service message is never a reply target"
    );
}

#[test]
fn telegram_topic_reply_without_topics_keeps_its_reply_target() {
    let reply = json!({
        "chat": { "id": CHAT },
        "message_thread_id": 900,
        "reply_to_message": { "message_id": 900 },
        "text": "yes",
    });
    assert_eq!(
        reply_context(&reply),
        ReplyContext::Telegram {
            chat_id: CHAT,
            thread_id: None,
            reply_to_message_id: Some(900),
        },
        "a reply thread in a non-forum chat is not a topic"
    );
}

#[test]
fn telegram_topic_plain_and_general_topic_messages_have_no_thread() {
    let no_thread = ReplyContext::Telegram {
        chat_id: CHAT,
        thread_id: None,
        reply_to_message_id: None,
    };
    assert_eq!(
        reply_context(&json!({ "chat": { "id": CHAT }, "text": "hello" })),
        no_thread
    );
    assert_eq!(
        reply_context(&json!({
            "chat": { "id": CHAT },
            "is_topic_message": false,
            "message_thread_id": 1,
            "text": "general topic",
        })),
        no_thread
    );
}

#[tokio::test]
async fn telegram_topic_first_tab_send_creates_one_topic_and_routes_it_back() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let placed = threads
        .route_text(&tabs, text(PLANCK, "Build is green"))
        .await
        .expect("placed");

    assert_eq!(transport.creates(), vec!["Planck".to_string()]);
    let post = topic_post(&transport);
    let thread = post.thread.expect("thread");
    assert_eq!(post.message, placed, "the topic post is the placement");
    let pointer = transport
        .sends()
        .into_iter()
        .find(|sent| sent.thread.is_none())
        .expect("a main-chat pointer");
    let header = tabs.header(PLANCK);
    assert!(post.html.contains(&header), "topic post names its sender");
    assert!(pointer.html.contains(&header), "pointer names its sender");
    assert_eq!(topic_link(CHAT, thread), None);
    assert!(
        !pointer.html.contains("href"),
        "a private-chat pointer carries no link that cannot open the topic"
    );

    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, Some(thread), None),
        InboundRoute::Tab {
            tab_id: PLANCK.to_string(),
            thread_id: Some(thread),
        }
    );
    let thread_attr = thread.to_string();
    let marker =
        message_markers::telegram_routed("42", Some("7"), Some(&thread_attr), None, "hello");
    let parsed = message_markers::parse(&marker).expect("marker parses");
    assert_eq!(parsed.kind, "telegram");
    assert_eq!(parsed.attr("thread"), Some(thread_attr.as_str()));
    assert_eq!(parsed.attr("origin"), None);
    assert_eq!(parsed.payload, "hello");
}

#[tokio::test]
async fn telegram_topic_repeated_sends_reuse_one_binding() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    for body in ["one", "two", "three"] {
        threads
            .route_text(&tabs, text(PLANCK, body))
            .await
            .expect("placed");
    }

    assert_eq!(transport.creates().len(), 1, "one topic per tab");
    let sends = transport.sends();
    let topic_threads: Vec<_> = sends.iter().filter_map(|sent| sent.thread).collect();
    assert_eq!(topic_threads.len(), 3);
    assert!(topic_threads.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(sends.iter().filter(|sent| sent.thread.is_none()).count(), 3);
}

#[tokio::test]
async fn telegram_topic_choice_buttons_ride_the_topic_post() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let placed = threads
        .route_text(
            &tabs,
            TabText {
                buttons: Some(choice_keyboard()),
                ..text(PLANCK, "Deploy?")
            },
        )
        .await
        .expect("placed");

    let post = topic_post(&transport);
    assert!(post.buttons, "the choices ride the topic post");
    assert_eq!(
        post.message, placed,
        "the receipt names the message with buttons"
    );
    assert!(transport
        .sends()
        .iter()
        .filter(|sent| sent.thread.is_none())
        .all(|pointer| !pointer.buttons));
}

struct RecordingHost(Mutex<Vec<OutboundMessage>>);

impl ChannelHost for RecordingHost {
    fn service_ready(&self, _service: &str) -> bool {
        true
    }

    fn enqueue(&self, _service: &str, message: OutboundMessage) -> Result<(), String> {
        self.0.lock().push(message);
        Ok(())
    }
}

#[test]
fn telegram_topic_notification_from_a_tab_is_routed_through_that_tab() {
    let host = RecordingHost(Mutex::new(Vec::new()));
    let asked = PushMessage {
        text: "Deploy?".to_string(),
        choices: vec![ChannelChoice {
            label: "Ship it".to_string(),
            token: "na:record-1:0".to_string(),
        }],
        record: "record-1".to_string(),
        origin_tab: Some(PLANCK.to_string()),
    };
    crate::notification::channels::telegram::CHANNEL
        .deliver(&host, &Audience(vec![CHAT.to_string()]), &asked)
        .expect("delivered");
    let sent = host.0.lock().pop().expect("enqueued");
    assert_eq!(
        sent.route,
        OutboundRoute::Tab {
            tab_id: PLANCK.to_string()
        }
    );
    assert!(sent.buttons.is_some(), "choices are preserved");
    assert!(sent.receipt.is_some(), "the placement still comes back");

    let unattributed = PushMessage {
        origin_tab: None,
        ..asked
    };
    crate::notification::channels::telegram::CHANNEL
        .deliver(&host, &Audience(vec![CHAT.to_string()]), &unattributed)
        .expect("delivered");
    assert_eq!(
        host.0.lock().pop().expect("enqueued").route,
        OutboundRoute::Chat
    );
}

#[tokio::test]
async fn telegram_topic_sender_headers_name_all_four_roles() {
    assert_eq!(
        SenderRole::classify(false, Some("orchestrator")),
        SenderRole::Orchestrator
    );
    assert_eq!(
        SenderRole::classify(false, Some("planner")),
        SenderRole::Planner
    );
    assert_eq!(SenderRole::classify(false, None), SenderRole::Worker);
    assert_eq!(
        SenderRole::classify(false, Some("rust-platform-dev")),
        SenderRole::Worker
    );
    assert_eq!(SenderRole::classify(true, None), SenderRole::General);
    assert_eq!(
        SenderRole::classify(false, Some("general_agent")),
        SenderRole::General
    );

    for (role, label) in [
        (SenderRole::Orchestrator, "orchestrator"),
        (SenderRole::Worker, "worker"),
        (SenderRole::Planner, "planner"),
        (SenderRole::General, "general"),
    ] {
        let identity = SenderIdentity {
            tab_id: "tab_x".to_string(),
            title: "Planck".to_string(),
            role,
            workspace: "codeterm".to_string(),
            machine: "BLACKSTAR".to_string(),
        };
        assert_eq!(
            identity.header(),
            ["Planck", label, "codeterm", "BLACKSTAR"].join(HEADER_SEPARATOR)
        );
    }

    let transport = FakeTransport::enabled();
    let tabs = FakeTabs::new();
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text(GENERAL, "main chat reply"))
        .await
        .expect("placed");
    assert!(
        transport.creates().is_empty(),
        "the General Agent has no topic"
    );
    let sends = transport.sends();
    assert_eq!(sends.len(), 1, "no pointer for a main-chat send");
    assert_eq!(sends[0].thread, None);
    assert!(sends[0].html.contains(&tabs.header(GENERAL)));
}

#[tokio::test]
async fn telegram_topic_duplicate_live_titles_get_a_short_deterministic_suffix() {
    let alone = vec![("tab_aaaa1111".to_string(), "Planck".to_string())];
    assert_eq!(topic_name("Planck", "tab_aaaa1111", &alone), "Planck");

    let twins = vec![
        ("tab_aaaa1111".to_string(), "Planck".to_string()),
        ("tab_bbbb2222".to_string(), "Planck".to_string()),
    ];
    let first = topic_name("Planck", "tab_aaaa1111", &twins);
    let second = topic_name("Planck", "tab_bbbb2222", &twins);
    assert!(first.starts_with("Planck") && second.starts_with("Planck"));
    assert_ne!(first, "Planck");
    assert_ne!(first, second);
    assert_eq!(
        first,
        topic_name("Planck", "tab_aaaa1111", &twins),
        "deterministic"
    );
    assert!(
        first.chars().count() <= "Planck".chars().count() + 8,
        "short suffix"
    );

    let transport = FakeTransport::enabled();
    let tabs = FakeTabs::new();
    tabs.add("tab_aaaa1111", "Planck", SenderRole::Worker);
    tabs.add("tab_bbbb2222", "Planck", SenderRole::Worker);
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text("tab_aaaa1111", "a"))
        .await
        .expect("placed");
    threads
        .route_text(&tabs, text("tab_bbbb2222", "b"))
        .await
        .expect("placed");
    let names = transport.creates();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
}

#[tokio::test]
async fn telegram_topic_disabled_falls_back_to_main_chat_with_one_notice() {
    let transport = FakeTransport::topics_disabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let first = threads
        .route_text(
            &tabs,
            TabText {
                buttons: Some(choice_keyboard()),
                ..text(PLANCK, "Deploy?")
            },
        )
        .await
        .expect("placed in the main chat");
    threads
        .route_text(&tabs, text(PLANCK, "again"))
        .await
        .expect("placed in the main chat");

    assert_eq!(
        transport.creates().len(),
        1,
        "no creation retry per message"
    );
    assert_eq!(transport.notices(), 1, "the owner is told once");
    let sends = transport.sends();
    assert!(sends.iter().all(|sent| sent.thread.is_none()));
    let carried = sends
        .iter()
        .find(|sent| sent.message == first)
        .expect("first send placed");
    assert!(carried.buttons, "choices survive the fallback");
    assert!(carried.html.contains(&tabs.header(PLANCK)));
    assert_eq!(
        threads.menu_state(PLANCK, Some(CHAT)).await.topics,
        TopicsState::Unavailable
    );

    threads.attach(transport.clone(), bot_id_from_token(TOKEN));
    threads
        .route_text(&tabs, text(PLANCK, "after restart"))
        .await
        .expect("placed");
    assert_eq!(
        transport.creates().len(),
        2,
        "a bridge restart probes once more"
    );
    assert_eq!(transport.notices(), 1, "the notice is never repeated");
}

#[tokio::test]
async fn telegram_topic_reply_to_a_tab_message_routes_without_topics() {
    let transport = FakeTransport::topics_disabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport);

    let placed = threads
        .route_text(&tabs, text(PLANCK, "question for you"))
        .await
        .expect("placed");

    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, None, Some(placed)),
        InboundRoute::Tab {
            tab_id: PLANCK.to_string(),
            thread_id: None,
        }
    );
    let general = InboundRoute::General {
        thread_id: None,
        origin: None,
    };
    assert_eq!(threads.resolve_inbound(&tabs, CHAT, None, Some(1)), general);
    assert_eq!(threads.resolve_inbound(&tabs, CHAT, None, None), general);
}

#[tokio::test]
async fn telegram_topic_closed_tab_and_unbound_threads_reach_general_with_origin() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text(PLANCK, "hi"))
        .await
        .expect("placed");
    let thread = topic_post(&transport).thread.expect("thread");

    tabs.close(PLANCK);
    let route = threads.resolve_inbound(&tabs, CHAT, Some(thread), None);
    let origin = InboundOrigin::ClosedTab {
        tab_id: PLANCK.to_string(),
    };
    assert_eq!(
        route,
        InboundRoute::General {
            thread_id: Some(thread),
            origin: Some(origin.clone()),
        },
        "a gone tab's thread never routes to another tab"
    );
    let marker = message_markers::telegram_routed(
        "42",
        None,
        Some(&thread.to_string()),
        Some(origin.marker_value()),
        "hello",
    );
    assert_eq!(
        message_markers::parse(&marker)
            .expect("marker parses")
            .attr("origin"),
        Some(origin.marker_value())
    );

    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, Some(424_242), None),
        InboundRoute::General {
            thread_id: Some(424_242),
            origin: Some(InboundOrigin::UnboundThread),
        }
    );
}

#[tokio::test]
async fn telegram_topic_tab_close_deletes_the_topic_and_a_same_title_tab_gets_a_fresh_one() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text(PLANCK, "hi"))
        .await
        .expect("placed");
    let first_thread = topic_post(&transport).thread.expect("thread");

    let outcomes = threads.close_tab(PLANCK).await;

    assert_eq!(
        outcomes,
        vec![CloseOutcome::Deleted {
            thread_id: first_thread
        }]
    );
    assert!(transport.calls().contains(&Call::Delete {
        thread: first_thread
    }));
    assert!(!transport
        .calls()
        .iter()
        .any(|call| matches!(call, Call::Rename { .. })));
    assert!(!threads.has_open_binding(PLANCK));

    tabs.close(PLANCK);
    tabs.add("tab_planck_next", "Planck", SenderRole::Worker);
    threads
        .route_text(&tabs, text("tab_planck_next", "fresh"))
        .await
        .expect("placed");
    assert_eq!(
        transport.creates(),
        vec!["Planck".to_string(), "Planck".to_string()]
    );
    let fresh_thread = transport
        .sends()
        .into_iter()
        .rev()
        .find_map(|sent| sent.thread)
        .expect("fresh thread");
    assert_ne!(fresh_thread, first_thread);
    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, Some(first_thread), None),
        InboundRoute::General {
            thread_id: Some(first_thread),
            origin: Some(InboundOrigin::UnboundThread),
        }
    );
}

#[tokio::test]
async fn telegram_topic_failed_delete_renames_with_closed_prefix_and_routes_general() {
    let transport = FakeTransport::failing_delete();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text(PLANCK, "hi"))
        .await
        .expect("placed");
    let thread = topic_post(&transport).thread.expect("thread");
    let before_close = transport.calls().len();

    let outcomes = threads.close_tab(PLANCK).await;

    assert_eq!(
        outcomes,
        vec![CloseOutcome::Renamed {
            thread_id: thread,
            renamed: true
        }]
    );
    let closing_calls = transport.calls()[before_close..].to_vec();
    assert!(matches!(closing_calls.first(), Some(Call::Delete { thread: t }) if *t == thread));
    assert!(
        closing_calls
            .iter()
            .any(|call| matches!(call, Call::Send(sent) if sent.thread == Some(thread))),
        "a closing note is posted into the topic"
    );
    let renamed = closing_calls
        .iter()
        .find_map(|call| match call {
            Call::Rename { thread: t, name } if *t == thread => Some(name.clone()),
            _ => None,
        })
        .expect("the topic is renamed");
    assert_ne!(renamed, "Planck");
    assert!(
        renamed.ends_with("Planck"),
        "the closed prefix keeps the name"
    );
    assert_eq!(
        closing_calls.len(),
        3,
        "delete, note and rename only; never closeForumTopic"
    );
    assert!(!threads.has_open_binding(PLANCK));

    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, Some(thread), None),
        InboundRoute::General {
            thread_id: Some(thread),
            origin: Some(InboundOrigin::ClosedThread {
                topic_name: renamed
            }),
        }
    );
}

#[tokio::test]
async fn telegram_topic_deleted_thread_send_falls_back_and_rebinds_next_time() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text(PLANCK, "hi"))
        .await
        .expect("placed");
    let gone = topic_post(&transport).thread.expect("thread");
    transport.gone_threads.lock().push(gone);

    let placed = threads
        .route_text(&tabs, text(PLANCK, "still here"))
        .await
        .expect("placed in the main chat");
    let fallback = transport
        .sends()
        .into_iter()
        .find(|sent| sent.message == placed)
        .expect("fallback send");
    assert_eq!(fallback.thread, None);
    assert!(fallback.html.contains(&tabs.header(PLANCK)));
    assert!(
        !threads.has_open_binding(PLANCK),
        "a deleted thread is unbound"
    );

    threads
        .route_text(&tabs, text(PLANCK, "again"))
        .await
        .expect("placed");
    assert_eq!(transport.creates().len(), 2);
}

#[tokio::test]
async fn telegram_topic_append_edits_the_tab_post_without_a_new_pointer() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    let placed = threads
        .route_text(&tabs, text(PLANCK, "part one"))
        .await
        .expect("placed");
    let sends_before = transport.sends().len();

    let appended = threads
        .route_text(
            &tabs,
            TabText {
                append: true,
                ..text(PLANCK, "part two")
            },
        )
        .await;

    assert_eq!(appended, Some(placed));
    assert!(transport.calls().contains(&Call::Edit { message: placed }));
    assert_eq!(
        transport.sends().len(),
        sends_before,
        "no new post or pointer"
    );
}

#[tokio::test]
async fn telegram_topic_binding_persists_across_restarts_without_credentials() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("bridge").join("telegram-threads.json");

    let first_transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let first = attached_with(ThreadRegistry::load(path.clone()), first_transport.clone());
    first
        .route_text(&tabs, text(PLANCK, "hi"))
        .await
        .expect("placed");
    let thread = topic_post(&first_transport).thread.expect("thread");

    let second_transport = FakeTransport::enabled();
    let second = attached_with(ThreadRegistry::load(path.clone()), second_transport.clone());
    second
        .route_text(&tabs, text(PLANCK, "after restart"))
        .await
        .expect("placed");
    assert!(
        second_transport.creates().is_empty(),
        "the stored binding is reused"
    );
    assert_eq!(topic_post(&second_transport).thread, Some(thread));

    let stored = std::fs::read_to_string(&path).expect("binding persisted");
    assert!(stored.contains(PLANCK));
    let secret = TOKEN.split(':').nth(1).expect("secret half");
    for forbidden in [TOKEN, secret, "api.telegram.org", "http", "7685", "7686"] {
        assert!(
            !stored.contains(forbidden),
            "stored bindings leak `{forbidden}`"
        );
    }
}

#[test]
fn telegram_topic_failure_classification_and_links() {
    assert_eq!(
        classify_failure("Bad Request: the chat is not a forum"),
        TopicFailure::TopicsUnavailable
    );
    assert_eq!(
        classify_failure("Bad Request: message thread not found"),
        TopicFailure::ThreadGone
    );
    assert!(matches!(
        classify_failure("Bad Request: not enough rights"),
        TopicFailure::Rejected(_)
    ));
    assert_eq!(bot_id_from_token(TOKEN), 777_000);
}
