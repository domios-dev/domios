use super::*;
use crate::{
    multiplexer::state::MultiplexerState, ports::daemon_question::DaemonQuestionHandler,
    service::test_fixture::ServiceFixture,
};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

struct Handler {
    calls: AtomicUsize,
    refuse: AtomicBool,
}
impl DaemonQuestionHandler for Handler {
    fn answer(&self, request: &str, label: &str) -> Option<Result<serde_json::Value, String>> {
        request.strip_prefix("fixture:")?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        Some(if self.refuse.load(Ordering::SeqCst) {
            Err("request no longer available".into())
        } else {
            Ok(serde_json::json!({"recorded":label}))
        })
    }
}

#[test]
fn daemon_owner_question_deduplicates_and_records_only_an_admitted_offered_decision() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let handler = Arc::new(Handler {
        calls: AtomicUsize::new(0),
        refuse: AtomicBool::new(true),
    });
    assert!(fixture
        .service
        .register_host_extension(Arc::new(DaemonQuestions::new(handler.clone())))
        .is_ok());
    let choices = vec!["Continue".into(), "Review".into()];
    let id = fixture
        .service
        .publish_daemon_question(
            "fixture:one".into(),
            "Owner decision".into(),
            "Bounded operation".into(),
            choices.clone(),
        )
        .unwrap();
    assert_eq!(
        fixture
            .service
            .publish_daemon_question(
                "fixture:one".into(),
                "Owner decision".into(),
                "Bounded operation".into(),
                choices
            )
            .unwrap(),
        id
    );
    assert!(fixture
        .service
        .answer_notification(&id, "Continue")
        .is_err());
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    assert!(fixture
        .service
        .answer_notification_as_owner(&id, "unoffered")
        .is_err());
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    assert!(fixture
        .service
        .answer_notification_as_owner(&id, "Continue")
        .is_err());
    assert!(fixture
        .service
        .notif
        .lock()
        .answer_slot(&id)
        .unwrap()
        .answered
        .is_none());
    handler.refuse.store(false, Ordering::SeqCst);
    assert_eq!(
        fixture
            .service
            .answer_notification_as_owner(&id, "Review")
            .unwrap()["recorded"],
        "Review"
    );
    let repeated = fixture
        .service
        .answer_notification_as_owner(&id, "Continue")
        .unwrap();
    assert_eq!(repeated["answered"], "Review");
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn daemon_owner_question_claims_exclude_concurrent_choices_and_release_on_drop() {
    let questions = Arc::new(DaemonQuestions::new(Arc::new(Handler {
        calls: AtomicUsize::new(0),
        refuse: AtomicBool::new(false),
    })));
    let claim = questions.claim("one").unwrap();
    assert!(questions.claim("one").is_none());
    assert!(questions.claim("other").is_some());
    drop(claim);
    assert!(questions.claim("one").is_some());
}
