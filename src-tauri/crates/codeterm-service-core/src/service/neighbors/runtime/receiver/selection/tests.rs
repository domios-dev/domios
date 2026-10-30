use super::*;

#[derive(Default)]
struct Port {
    trace: parking_lot::Mutex<Vec<Value>>,
    fail_enrollment: bool,
}
#[async_trait::async_trait]
impl SelectionPort for Port {
    async fn enroll(&self) -> codeterm_neighbors::Result<()> {
        self.trace.lock().push(json!("enroll"));
        if self.fail_enrollment {
            Err(Error::Transport)
        } else {
            Ok(())
        }
    }
    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value> {
        self.trace.lock().push(command);
        Ok(json!({"selected":true}))
    }
}

#[tokio::test]
async fn fresh_friend_setup_enrolls_before_selecting_only_that_friend() {
    let port = Port::default();
    select_friend_receiver(&port, "here", "friend", Selection::Set, 0, false)
        .await
        .unwrap();
    let trace = port.trace.lock();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[0], "enroll");
    assert_eq!(trace[1]["friendship_id"], "friend");
    assert_eq!(trace[1]["expected_revision"], 0);
    assert_eq!(trace[1]["takeover"], false);
    assert_eq!(trace[1]["operation"], "set");
    assert!(trace[1].get("require_no_receivers").is_none());
}

#[tokio::test]
async fn enrollment_failure_never_changes_receiving_and_reset_never_enrolls() {
    let port = Port {
        fail_enrollment: true,
        ..Default::default()
    };
    assert_eq!(
        select_friend_receiver(&port, "here", "friend", Selection::Set, 7, true).await,
        Err(Error::Transport)
    );
    assert_eq!(*port.trace.lock(), vec![json!("enroll")]);
    let reset = Port::default();
    select_friend_receiver(&reset, "here", "friend", Selection::Reset, 7, false)
        .await
        .unwrap();
    let trace = reset.trace.lock();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["operation"], "reset");
    assert_eq!(trace[0]["expected_revision"], 7);
}

#[test]
fn moving_a_friend_requires_explicit_consent_and_the_exact_friend_revision() {
    let status = ReceiverStatus {
        receiver_revision: 7,
        receiver_installation_id: Some("elsewhere".into()),
        receiver_machine_label: Some("Other desk".into()),
        ..Default::default()
    };
    assert_eq!(
        validate_selection(&status, 7, false),
        Err(Error::ReceiverElsewhere)
    );
    assert_eq!(
        validate_selection(&status, 6, true),
        Err(Error::ReceiverRevisionStale)
    );
    assert_eq!(validate_selection(&status, 7, true), Ok(()));
}
