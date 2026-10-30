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
    let token = channel_ask::encode(&id, 0);
    let callback = |update: i64, user: i64, data: &str| {
        serde_json::json!({"update_id":update,"callback_query":{
            "id":update.to_string(),"from":{"id":user},"data":data,"message":{"message_id":7,"chat":{"id":42}}
        }})
    };
    let root = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let poll = server.mock("POST", "/botfixture/getUpdates")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({"offset":0})))
        .with_header("content-type", "application/json")
        .with_body(serde_json::json!({"ok":true,"result":[callback(1,222,&token),callback(2,111,&token),callback(3,111,&token)]}).to_string())
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
        ]))
        .with_body("{\"ok\":true}")
        .expect(2)
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
    for _ in 0..2 {
        let tap = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(tap.user_id, 111);
        assert_eq!(
            fixture
                .service
                .claim_channel_answer(tap.callback_data.as_deref().unwrap()),
            Some(Ok(()))
        );
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
    assert!(fixture
        .service
        .claim_channel_answer(&channel_ask::encode(&id, 9))
        .unwrap()
        .is_err());
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .service
            .claim_channel_answer(&channel_ask::encode(&id, 0)),
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
