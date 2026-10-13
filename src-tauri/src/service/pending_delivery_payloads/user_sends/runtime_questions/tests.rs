use super::*;
use codeterm_plugin::agent_runtime::{
    RuntimeQuestion, RuntimeQuestionAnswer, RuntimeQuestionResponse,
};

fn record() -> QuestionRecord {
    QuestionRecord {
        thread_id: "thread".into(),
        item_id: "item".into(),
        created_at: 100,
        questions: vec![RuntimeQuestion {
            id: "q".into(),
            text: "opaque".into(),
            options: vec![codeterm_types::pty::PromptOption {
                label: "display".into(),
                value: "native".into(),
            }],
            multi_select: None,
            allow_free_text: None,
        }],
        response: RuntimeQuestionResponse::Message,
        resolved: None,
    }
}
fn reserve(store: &UserSendStore, sequence: u64) -> UserReservation {
    store
        .reserve(
            UserSendAuthority {
                tab_id: "tab".into(),
                pty_id: None,
                binding_nonce: "nonce".into(),
                pty_session_generation: None,
                provider_id: "provider".into(),
                session_id: Some("thread".into()),
                observed_session_id: Some("thread".into()),
                prelaunch: false,
            },
            Some(format!("token-{sequence}")),
            || (uuid::Uuid::new_v4().to_string(), sequence),
        )
        .unwrap()
}
fn resolution(reservation: &UserReservation) -> QuestionResolution {
    QuestionResolution {
        thread_id: "thread".into(),
        item_id: "item".into(),
        receipt: QuestionReceipt {
            answers: vec![RuntimeQuestionAnswer {
                question_id: "q".into(),
                values: vec!["native".into()],
                text: None,
            }],
            send_id: reservation.send_id.clone(),
            status: serde_json::json!({"send_id":reservation.send_id,"state":"held"}),
        },
    }
}
#[test]
fn answer_receipt_and_user_admission_restore_together_and_repeat_cannot_admit() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    store.persist_runtime_question("tab", record()).unwrap();
    let reservation = reserve(&store, 1);
    let send = store
        .admit_question_answer(
            &reservation,
            "opaque-answer",
            resolution(&reservation),
            false,
        )
        .unwrap();
    drop(reservation);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let questions = reopened.runtime_questions("tab", "thread").unwrap();
    assert_eq!(
        questions[0].resolved.as_ref().unwrap().send_id,
        send.send_id
    );
    assert_eq!(reopened.records("tab").unwrap().len(), 1);
    assert_eq!(reopened.prepared(&send).unwrap(), "opaque-answer");
    let repeat = reserve(&reopened, 2);
    assert!(reopened
        .admit_question_answer(&repeat, "duplicate", resolution(&repeat), false)
        .is_err());
    assert_eq!(reopened.records("tab").unwrap().len(), 1);
    reopened.persist_runtime_question("tab", record()).unwrap();
    assert_eq!(
        reopened.runtime_questions("tab", "thread").unwrap()[0]
            .resolved
            .as_ref()
            .unwrap()
            .send_id,
        send.send_id
    );
}
#[test]
fn persistence_failure_never_commits_a_question_resolution_without_admission() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    store.persist_runtime_question("tab", record()).unwrap();
    store.set_persistence_boundary(Some(Arc::new(|stage| {
        if stage == "index" {
            Err("injected".into())
        } else {
            Ok(())
        }
    })));
    let reservation = reserve(&store, 1);
    assert!(store
        .admit_question_answer(
            &reservation,
            "opaque-answer",
            resolution(&reservation),
            false
        )
        .is_err());
    drop(reservation);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.runtime_questions("tab", "thread").unwrap()[0]
        .resolved
        .is_none());
    assert!(reopened.records("tab").unwrap().is_empty());
}
