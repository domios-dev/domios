//! One host attach operation for every caller: idempotent binding, one intro, chat-kind links.

use codeterm_bridge::bridge::threads::transport::{chat_kind, probe_outcome, ChatKind};
use codeterm_bridge::bridge::threads::{OpenedThread, ThreadError};

use super::*;

fn topic_posts(transport: &FakeTransport, thread_id: i64) -> usize {
    transport
        .sends()
        .iter()
        .filter(|sent| sent.thread == Some(thread_id))
        .count()
}

#[tokio::test]
async fn telegram_topic_attach_is_idempotent_and_posts_the_intro_once() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let first = threads
        .open(&tabs, tab_caller(PLANCK), PLANCK, CHAT)
        .await
        .expect("opened");
    let second = threads
        .open(&tabs, tab_caller(PLANCK), PLANCK, CHAT)
        .await
        .expect("reopened");

    assert!(first.created);
    assert_eq!(
        second,
        OpenedThread {
            created: false,
            ..first.clone()
        }
    );
    assert_eq!(transport.creates().len(), 1);
    assert_eq!(topic_posts(&transport, first.thread_id), 1);
    let intro = topic_post(&transport);
    assert!(intro.html.contains("Planck") && intro.html.contains(&tabs.header(PLANCK)));
}

#[tokio::test]
async fn telegram_topic_attach_menu_general_agent_and_own_tab_share_one_binding() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let from_general = threads
        .open(&tabs, tab_caller(GENERAL), PLANCK, CHAT)
        .await
        .expect("General Agent open");
    let from_menu = threads
        .open(&tabs, ThreadCaller::Owner, PLANCK, CHAT)
        .await
        .expect("menu open");
    let from_own_tab = threads
        .open(&tabs, tab_caller(PLANCK), PLANCK, CHAT)
        .await
        .expect("own-tab open");

    assert!(from_general.created);
    for later in [&from_menu, &from_own_tab] {
        assert_eq!(
            *later,
            OpenedThread {
                created: false,
                ..from_general.clone()
            }
        );
    }
    assert_eq!(transport.creates().len(), 1);
    assert_eq!(topic_posts(&transport, from_general.thread_id), 1);
    assert!(threads.has_open_binding(PLANCK));
    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, Some(from_general.thread_id), None),
        InboundRoute::Tab {
            tab_id: PLANCK.to_string(),
            thread_id: Some(from_general.thread_id),
        }
    );
    assert_eq!(
        threads.menu_state(PLANCK, Some(CHAT)).await.link,
        from_general.link
    );
}

#[tokio::test]
async fn telegram_topic_attach_concurrent_opens_create_and_post_once() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let (from_menu, from_general) = tokio::join!(
        threads.open(&tabs, ThreadCaller::Owner, PLANCK, CHAT),
        threads.open(&tabs, tab_caller(GENERAL), PLANCK, CHAT),
    );
    let from_menu = from_menu.expect("menu open");
    let from_general = from_general.expect("General Agent open");

    assert_eq!(from_menu.thread_id, from_general.thread_id);
    assert_eq!(transport.creates().len(), 1);
    assert_eq!(topic_posts(&transport, from_menu.thread_id), 1);
}

#[tokio::test]
async fn telegram_topic_attach_reuses_a_topic_a_send_created() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    threads
        .route_text(&tabs, text(PLANCK, "hi"))
        .await
        .expect("placed");

    let opened = threads
        .open(&tabs, tab_caller(PLANCK), PLANCK, CHAT)
        .await
        .expect("opened");

    assert!(!opened.created);
    assert_eq!(
        transport.creates().len(),
        1,
        "one binding owner for sends and opens"
    );
    assert_eq!(Some(opened.thread_id), topic_post(&transport).thread);
}

#[tokio::test]
async fn telegram_topic_attach_general_agent_opens_a_topic_owned_by_the_target() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());

    let opened = threads
        .open(&tabs, tab_caller(GENERAL), PLANCK, CHAT)
        .await
        .expect("the General Agent may name another tab");

    assert_eq!(opened.tab_id, PLANCK);
    assert_eq!(transport.creates(), vec!["Planck".to_string()]);
    let intro = topic_post(&transport);
    assert!(intro.html.contains(&tabs.header(PLANCK)));
    assert!(!intro.html.contains(&tabs.header(GENERAL)));
}

#[tokio::test]
async fn telegram_topic_attach_foreign_target_is_refused_without_any_bot_api_call() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    tabs.add(ROWAN, "Rowan", SenderRole::Worker);
    let threads = attached(transport.clone());

    let refused = threads
        .open(&tabs, tab_caller(ROWAN), PLANCK, CHAT)
        .await
        .expect_err("a worker may open only its own thread");
    assert_eq!(
        refused,
        ThreadError::ForeignTarget {
            caller: ROWAN.to_string(),
            target: PLANCK.to_string(),
        }
    );
    assert!(refused.is_authority());
    let missing = threads
        .open(&tabs, tab_caller(ROWAN), "tab_missing", CHAT)
        .await
        .expect_err("authority is decided before existence");
    assert!(missing.is_authority());

    assert!(transport.calls().is_empty(), "no Bot API call");
    assert!(!threads.has_open_binding(PLANCK));
    assert!(
        !threads.stored_json().contains(PLANCK),
        "no binding recorded"
    );
}

#[tokio::test]
async fn telegram_topic_attach_general_tab_bridge_off_and_topics_off_are_explicit_errors() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    assert_eq!(
        threads
            .open(&tabs, ThreadCaller::Owner, GENERAL, CHAT)
            .await,
        Err(ThreadError::GeneralUsesMainChat)
    );
    assert!(transport.calls().is_empty());

    let detached = ThreadService::new(ThreadRegistry::in_memory());
    assert_eq!(
        detached
            .open(&tabs, ThreadCaller::Owner, PLANCK, CHAT)
            .await,
        Err(ThreadError::BridgeInactive)
    );

    let disabled = FakeTransport::topics_disabled();
    let threads = attached(disabled.clone());
    for _ in 0..2 {
        assert_eq!(
            threads.open(&tabs, tab_caller(PLANCK), PLANCK, CHAT).await,
            Err(ThreadError::TopicsUnavailable)
        );
    }
    assert_eq!(disabled.notices(), 1);
    assert_eq!(disabled.creates().len(), 2, "an explicit open probes again");
}

#[tokio::test]
async fn telegram_topic_attach_menu_state_reflects_bridge_topics_and_binding() {
    let tabs = tabs_with_planck();
    let detached = ThreadService::new(ThreadRegistry::in_memory());
    let off = detached.menu_state(PLANCK, Some(CHAT)).await;
    assert!(!off.bridge_active && !off.bound);
    assert_eq!(off.topics, TopicsState::Unknown);

    let transport = FakeTransport::enabled();
    let threads = attached(transport);
    let before = threads.menu_state(PLANCK, Some(CHAT)).await;
    assert!(before.bridge_active && !before.bound && before.link.is_none());

    let opened = threads
        .open(&tabs, ThreadCaller::Owner, PLANCK, CHAT)
        .await
        .expect("opened");
    let after = threads.menu_state(PLANCK, Some(CHAT)).await;
    assert!(after.bound);
    assert_eq!(after.topics, TopicsState::Available);
    assert_eq!(after.link, opened.link);
}

#[test]
fn telegram_topic_attach_link_forms_follow_the_chat_kind() {
    assert_eq!(chat_kind(CHAT), ChatKind::Private);
    assert_eq!(
        chat_kind(SUPERGROUP),
        ChatKind::Supergroup {
            internal_id: 1_234_567_890
        }
    );
    assert_eq!(chat_kind(-4_567_890), ChatKind::Group);
    assert_eq!(
        topic_link(SUPERGROUP, 5).as_deref(),
        Some("https://t.me/c/1234567890/5")
    );
    assert_eq!(topic_link(CHAT, 5), None, "no topic link in a private chat");
    assert_eq!(
        topic_link(-4_567_890, 5),
        None,
        "basic groups have no forum"
    );
}

#[tokio::test]
async fn telegram_topic_attach_emits_a_link_only_where_it_opens_the_topic() {
    let tabs = tabs_with_planck();

    let private = attached(FakeTransport::enabled());
    let opened = private
        .open(&tabs, ThreadCaller::Owner, PLANCK, CHAT)
        .await
        .expect("private-chat open");
    assert_eq!(opened.link, None);
    assert_eq!(private.menu_state(PLANCK, Some(CHAT)).await.link, None);

    let forum = attached(FakeTransport::enabled());
    let opened = forum
        .open(&tabs, ThreadCaller::Owner, PLANCK, SUPERGROUP)
        .await
        .expect("supergroup open");
    assert_eq!(opened.link, topic_link(SUPERGROUP, opened.thread_id));
    assert!(opened.link.is_some());
    assert_eq!(
        forum.menu_state(PLANCK, Some(SUPERGROUP)).await.link,
        opened.link
    );
}

#[test]
fn telegram_topic_attach_probe_and_send_errors_classify_topic_state() {
    assert_eq!(
        probe_outcome("Bad Request: TOPIC_NOT_MODIFIED"),
        Some(TopicProbe::Open)
    );
    for gone in [
        "Bad Request: message thread not found",
        "Bad Request: TOPIC_ID_INVALID",
        "Bad Request: TOPIC_DELETED",
    ] {
        assert_eq!(probe_outcome(gone), Some(TopicProbe::Gone), "{gone}");
        assert_eq!(classify_failure(gone), TopicFailure::ThreadGone, "{gone}");
    }
    assert_eq!(probe_outcome("Bad Request: not enough rights"), None);
    assert_eq!(
        classify_failure("Bad Request: TOPIC_CLOSED"),
        TopicFailure::ThreadClosed
    );
    assert!(TopicFailure::ThreadClosed.thread_unusable());
    assert!(!TopicFailure::Rejected("x".into()).thread_unusable());
}

async fn attach_planck(threads: &ThreadService, tabs: &FakeTabs) -> OpenedThread {
    threads
        .open(tabs, ThreadCaller::Owner, PLANCK, CHAT)
        .await
        .expect("attached")
}

#[tokio::test]
async fn telegram_topic_attach_recreates_a_deleted_topic_once_with_its_first_message() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    let first = attach_planck(&threads, &tabs).await;
    transport.gone_threads.lock().push(first.thread_id);

    let recreated = attach_planck(&threads, &tabs).await;
    let again = attach_planck(&threads, &tabs).await;

    assert!(recreated.created);
    assert_ne!(recreated.thread_id, first.thread_id);
    assert_eq!(
        again,
        OpenedThread {
            created: false,
            ..recreated.clone()
        }
    );
    assert_eq!(transport.creates().len(), 2);
    assert_eq!(topic_posts(&transport, recreated.thread_id), 1);
    assert_eq!(
        threads.resolve_inbound(&tabs, CHAT, Some(recreated.thread_id), None),
        InboundRoute::Tab {
            tab_id: PLANCK.to_string(),
            thread_id: Some(recreated.thread_id),
        }
    );
    assert!(threads.menu_state(PLANCK, Some(CHAT)).await.bound);
}

#[tokio::test]
async fn telegram_topic_attach_reopens_a_closed_topic_instead_of_duplicating_it() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    let first = attach_planck(&threads, &tabs).await;
    transport.closed_threads.lock().push(first.thread_id);

    let reopened = attach_planck(&threads, &tabs).await;

    assert_eq!(
        reopened,
        OpenedThread {
            created: false,
            ..first.clone()
        }
    );
    assert_eq!(transport.creates().len(), 1);
    assert!(transport.closed_threads.lock().is_empty(), "reopened");
    assert_eq!(
        topic_posts(&transport, first.thread_id),
        1,
        "no second intro"
    );
}

#[tokio::test]
async fn telegram_topic_attach_concurrent_attaches_after_deletion_create_one_topic() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    let first = attach_planck(&threads, &tabs).await;
    transport.gone_threads.lock().push(first.thread_id);

    let (from_menu, from_general) = tokio::join!(
        threads.open(&tabs, ThreadCaller::Owner, PLANCK, CHAT),
        threads.open(&tabs, tab_caller(GENERAL), PLANCK, CHAT),
    );
    let from_menu = from_menu.expect("menu attach");
    let from_general = from_general.expect("General Agent attach");

    assert_eq!(from_menu.thread_id, from_general.thread_id);
    assert_ne!(from_menu.thread_id, first.thread_id);
    assert_eq!(
        transport.creates().len(),
        2,
        "exactly one replacement topic"
    );
    assert_eq!(topic_posts(&transport, from_menu.thread_id), 1);
}

#[tokio::test]
async fn telegram_topic_attach_outbound_missing_thread_marks_the_binding_stale() {
    for closed in [false, true] {
        let transport = FakeTransport::enabled();
        let tabs = tabs_with_planck();
        let threads = attached(transport.clone());
        let first = attach_planck(&threads, &tabs).await;
        let dead = if closed {
            &transport.closed_threads
        } else {
            &transport.gone_threads
        };
        dead.lock().push(first.thread_id);

        threads
            .route_text(&tabs, text(PLANCK, "after the owner removed it"))
            .await
            .expect("placed in the main chat");

        let menu = threads.menu_state(PLANCK, Some(CHAT)).await;
        assert!(!menu.bound, "closed={closed}: a dead topic is not attached");
        assert!(!threads.has_open_binding(PLANCK));
        assert!(threads.holds_topics(PLANCK), "tab close still retires it");

        let reattached = attach_planck(&threads, &tabs).await;
        assert_eq!(reattached.created, !closed);
        assert_eq!(transport.creates().len(), if closed { 1 } else { 2 });
        assert!(threads.menu_state(PLANCK, Some(CHAT)).await.bound);
    }
}

#[tokio::test]
async fn telegram_topic_attach_caller_owned_media_failure_marks_only_thread_errors_stale() {
    let transport = FakeTransport::enabled();
    let tabs = tabs_with_planck();
    let threads = attached(transport.clone());
    let first = attach_planck(&threads, &tabs).await;

    threads.note_thread_send_failure(CHAT, first.thread_id, "Bad Request: not enough rights");
    assert!(threads.menu_state(PLANCK, Some(CHAT)).await.bound);

    threads.note_thread_send_failure(
        CHAT,
        first.thread_id,
        "Bad Request: message thread not found",
    );
    assert!(!threads.menu_state(PLANCK, Some(CHAT)).await.bound);
}
