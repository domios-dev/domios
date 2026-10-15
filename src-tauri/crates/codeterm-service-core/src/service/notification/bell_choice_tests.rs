use super::*;
use crate::notification::store::NotificationStore;
use parking_lot::Mutex;
use std::sync::Arc;

#[test]
fn bell_choice_reaches_the_asking_tab_once() {
    let store = Arc::new(Mutex::new(NotificationStore::new(10)));
    let id = create_notification_with_delivery(
        NotificationAdmission::Explicit,
        &store,
        |_, _| {},
        "question-token".into(),
        Some("detail-token".into()),
        None,
        Ask::offering(Some(vec![NotificationAction::Answer {
            label: "choice-token".into(),
        }])),
        None,
        NotificationOrigin::local_tab("asking-tab".into()),
        |candidate| {
            Some(NotificationProvenance {
                tab_id: Some(candidate.into()),
                ..Default::default()
            })
        },
        PushDispatch::inert(),
        SurfaceDispatch::silent(),
        |_| {},
    );
    assert!(matches!(
        store.lock().record_answer(&id, "choice-token"),
        AnswerAdmission::Recorded(_)
    ));
    let received = Mutex::new(Vec::new());
    for _ in 0..2 {
        answer_delivery::deliver(&store, &id, |tab, envelope, body| {
            received
                .lock()
                .push((tab.to_string(), envelope, body.to_string()));
            Ok(())
        });
    }
    let received = received.lock();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].0, "asking-tab");
    assert!(received[0].2.contains("choice-token"));
    assert!(store.lock().answer_slot(&id).unwrap().delivered);
}
