use super::*;
use codeterm_bridge::bridge::{telegram::TelegramService, traits::MessagingService};
use codeterm_bridge::ports::AttachmentStorePort;

struct Attachments(std::path::PathBuf);
impl AttachmentStorePort for Attachments {
    fn secrets_dir(&self) -> std::path::PathBuf {
        self.0.join("secrets")
    }
    fn evict_downloads(&self, _: &std::path::Path, _: u64) {}
    fn telegram_allowed_users(&self) -> Vec<String> {
        vec!["111".into()]
    }
}

#[tokio::test]
async fn daemon_channel_taps_authenticate_sender_and_answer_exactly_once() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let handler = Arc::new(Handler {
        calls: AtomicUsize::new(0),
        refuse: AtomicBool::new(false),
    });
    assert!(fixture
        .service
        .register_host_extension(Arc::new(DaemonQuestions::new(handler.clone())))
        .is_ok());
    let id = fixture
        .service
        .publish_daemon_question(
            "fixture:channel".into(),
            "Question".into(),
            "Opaque".into(),
            vec!["Continue".into()],
        )
        .unwrap();
    fixture
        .service
        .notif
        .lock()
        .remember_channel_placement(&id, "telegram", "42:7");
    let token = channel_ask::encode(&id, 0);
    let callback = |update: i64, user: i64, data: &str, chat: i64, message: i64| {
        serde_json::json!({"update_id":update,"callback_query":{
            "id":update.to_string(),"from":{"id":user},"data":data,"message":{"message_id":message,"chat":{"id":chat}}
        }})
    };
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let poll = server.mock("POST", "/botfixture/getUpdates")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({"offset":0})))
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"ok":true,"result":[callback(1,222,&token,42,7),callback(2,111,&token,42,99),callback(3,111,&token,43,7),callback(4,111,&token,42,7),callback(5,111,&token,42,7)]}).to_string())
        .expect(1).create_async().await;
    let denied = server
        .mock("POST", "/botfixture/answerCallbackQuery")
        .match_body(mockito::Matcher::PartialJson(
            serde_json::json!({"callback_query_id":"1","show_alert":true}),
        ))
        .with_body("{\"ok\":true}")
        .expect(1)
        .create_async()
        .await;
    let allowed = server
        .mock("POST", "/botfixture/answerCallbackQuery")
        .match_body(mockito::Matcher::AnyOf(vec![
            mockito::Matcher::Json(serde_json::json!({"callback_query_id":"2"})),
            mockito::Matcher::Json(serde_json::json!({"callback_query_id":"3"})),
            mockito::Matcher::Json(serde_json::json!({"callback_query_id":"4"})),
            mockito::Matcher::Json(serde_json::json!({"callback_query_id":"5"})),
        ]))
        .with_body("{\"ok\":true}")
        .expect(4)
        .create_async()
        .await;
    let service = TelegramService::with_base(
        "fixture".into(),
        server.url(),
        Arc::new(Attachments(root.path().into())),
    );
    let cancel = Arc::new(tokio::sync::Notify::new());
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let status = Arc::new(codeterm_bridge::bridge::supervisor::BridgeSupervisor::new());
    let task = tokio::spawn(Box::new(service).start(tx, cancel.clone(), status));
    for index in 0..4 {
        let tap = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tap.user_id, 111);
        let codeterm_bridge::bridge::traits::ReplyContext::Telegram { chat_id, .. } = tap.reply_ctx;
        let result = fixture
            .service
            .claim_channel_answer(
                tap.callback_data.as_deref().unwrap(),
                chat_id,
                tap.message_id,
            )
            .unwrap();
        if index < 2 {
            assert!(result.is_err());
            assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
            assert!(fixture
                .service
                .notif
                .lock()
                .answer_slot(&id)
                .unwrap()
                .answered
                .is_none());
        } else {
            assert_eq!(result, Ok(()));
        }
    }
    cancel.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(rx.try_recv().is_err());
    poll.assert_async().await;
    denied.assert_async().await;
    allowed.assert_async().await;
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert!(
        fixture
            .service
            .notif
            .lock()
            .answer_slot(&id)
            .unwrap()
            .delivered
    );
    assert!(fixture
        .service
        .answer_notification(&id, "Continue")
        .is_err());
}

#[test]
fn daemon_channel_taps_preserve_offered_choice_and_handler_refusals() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let handler = Arc::new(Handler {
        calls: AtomicUsize::new(0),
        refuse: AtomicBool::new(true),
    });
    assert!(fixture
        .service
        .register_host_extension(Arc::new(DaemonQuestions::new(handler.clone())))
        .is_ok());
    let id = fixture
        .service
        .publish_daemon_question(
            "fixture:channel-refusal".into(),
            "Question".into(),
            "Opaque".into(),
            vec!["Continue".into()],
        )
        .unwrap();
    fixture
        .service
        .notif
        .lock()
        .remember_channel_placement(&id, "telegram", "42:7");
    assert!(fixture
        .service
        .claim_channel_answer(&channel_ask::encode(&id, 9), 42, Some(7))
        .unwrap()
        .is_err());
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .service
            .claim_channel_answer(&channel_ask::encode(&id, 0), 42, Some(7)),
        Some(Err("request no longer available".into()))
    );
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    assert!(fixture
        .service
        .notif
        .lock()
        .answer_slot(&id)
        .unwrap()
        .answered
        .is_none());
}
