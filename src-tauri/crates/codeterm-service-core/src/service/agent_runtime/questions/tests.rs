use super::*;
fn record() -> QuestionRecord {
    QuestionRecord {
        thread_id: "own".into(),
        item_id: "item".into(),
        questions: vec![RuntimeQuestion {
            id: "question".into(),
            text: "opaque question".into(),
            options: vec![codeterm_types::pty::PromptOption {
                label: "opaque label".into(),
                value: "native-id".into(),
            }],
            multi_select: None,
            allow_free_text: Some(true),
        }],
        response: RuntimeQuestionResponse::Message,
        created_at: 123,
        resolved: None,
    }
}
fn receipt(id: &str) -> QuestionReceipt {
    QuestionReceipt {
        send_id: id.into(),
        answers: vec![RuntimeQuestionAnswer {
            question_id: "question".into(),
            values: vec!["native-id".into()],
            text: None,
        }],
        status: serde_json::json!({"send_id":id,"state":"held"}),
    }
}
#[test]
fn persisted_message_question_and_receipt_survive_restore_and_repeat_cannot_send_twice() {
    let mut book = QuestionBook::default();
    book.reconcile(record());
    book.reconcile(record());
    assert_eq!(book.list("own").len(), 1);
    assert_eq!(
        book.resolve("own", "item", receipt("first"))
            .unwrap()
            .send_id,
        "first"
    );
    let data = serde_json::to_vec(&book).unwrap();
    let mut restored: QuestionBook = serde_json::from_slice(&data).unwrap();
    restored.reconcile(record());
    assert_eq!(
        restored
            .resolve("own", "item", receipt("repeat"))
            .unwrap()
            .send_id,
        "first"
    );
    assert!(restored.list("foreign").is_empty());
}
#[test]
fn native_option_value_survives_normalization_and_a_label_is_not_an_answer() {
    let question = record();
    let correct = receipt("first");
    assert!(validate_answers(&question.questions, &correct.answers).is_ok());
    let mut by_label = correct.answers;
    by_label[0].values[0] = question.questions[0].options[0].label.clone();
    assert!(validate_answers(&question.questions, &by_label).is_err());
    let serialized = serde_json::to_value(&question).unwrap();
    assert_eq!(
        serialized["questions"][0]["options"][0]["value"],
        "native-id"
    );
}
#[test]
fn absent_history_page_does_not_remove_an_unresolved_question_and_other_threads_cannot_answer() {
    let mut book = QuestionBook::default();
    book.reconcile(record());
    let retained: QuestionBook =
        serde_json::from_slice(&serde_json::to_vec(&book).unwrap()).unwrap();
    assert!(retained.get("own", "item").unwrap().resolved.is_none());
    assert!(book.resolve("foreign", "item", receipt("wrong")).is_err());
    assert_eq!(book.list("own").len(), 1);
}

#[test]
fn history_answer_proof_does_not_fabricate_a_send_receipt_or_erase_owned_receipt() {
    let mut book = QuestionBook::default();
    book.reconcile_history(record(), true);
    assert!(book.list("own").is_empty());
    assert!(book.get("own", "item").unwrap().resolved.is_none());
    let mut owned = QuestionBook::default();
    owned.reconcile(record());
    owned.resolve("own", "item", receipt("own-send")).unwrap();
    owned.reconcile_history(record(), true);
    assert_eq!(
        owned.list("own")[0].resolved.as_ref().unwrap().send_id,
        "own-send"
    );
}
