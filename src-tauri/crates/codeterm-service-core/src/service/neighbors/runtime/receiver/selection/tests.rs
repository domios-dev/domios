use super::*;

struct Port {
    account_installation: Option<&'static str>,
    fail_enrollment: bool,
    trace: parking_lot::Mutex<Vec<Value>>,
}

#[async_trait::async_trait]
impl SelectionPort for Port {
    async fn account_receiver(&self) -> codeterm_neighbors::Result<ReceiverStatus> {
        self.trace.lock().push(json!({"call":"account_status"}));
        Ok(ReceiverStatus {
            receiver_revision: 3,
            receiver_installation_id: self.account_installation.map(str::to_owned),
            receiver_key_id: Some("account-key".into()),
            receiver_machine_label: Some("Account machine".into()),
            receiving_enabled: self.account_installation.is_some(),
            is_me: self.account_installation == Some("here"),
            legacy_active: false,
        })
    }

    async fn enroll(
        &self,
        enrollment: codeterm_neighbors::adapter::Enrollment,
    ) -> codeterm_neighbors::Result<()> {
        self.trace.lock().push(
            json!({"call":"enroll","installation_only":enrollment.installation_only,
            "takeover":enrollment.takeover,"revision":enrollment.expected_revision}),
        );
        if self.fail_enrollment {
            return Err(Error::KeyLimit {
                limit: 5,
                active: 5,
            });
        }
        Ok(())
    }

    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value> {
        self.trace
            .lock()
            .push(json!({"call":"friendsSetFriendReceiver","command":command}));
        Ok(json!({"selected":true}))
    }
}

#[tokio::test]
async fn set_enrolls_only_the_secondary_installation_then_moves_the_named_friend() {
    let friendship = "a".repeat(64);
    for (account_installation, installation_only) in [("elsewhere", true), ("here", false)] {
        let port = Port {
            account_installation: Some(account_installation),
            fail_enrollment: false,
            trace: Default::default(),
        };
        assert_eq!(
            select_friend_receiver(&port, "here", &friendship, Selection::Set, 9, true)
                .await
                .unwrap(),
            json!({"selected":true})
        );
        let trace = port.trace.lock();
        assert_eq!(trace.len(), 3);
        assert_eq!(trace[0], json!({"call":"account_status"}));
        assert_eq!(
            trace[1],
            json!({"call":"enroll","installation_only":installation_only,"takeover":false,"revision":3})
        );
        assert_eq!(trace[2]["call"], "friendsSetFriendReceiver");
        let mut command = trace[2]["command"].clone();
        let operation_id = command
            .as_object_mut()
            .unwrap()
            .remove("operation_id")
            .unwrap();
        assert!(operation_id.as_str().is_some_and(|id| hex_id(id, 32)));
        assert_eq!(
            command,
            json!({"installation_id":"here","friendship_id":friendship,
            "operation":"set","takeover":true,"expected_revision":9})
        );
    }
}

#[tokio::test]
async fn reset_skips_key_enrollment_and_submits_only_the_friend_reset() {
    let port = Port {
        account_installation: Some("elsewhere"),
        fail_enrollment: true,
        trace: Default::default(),
    };
    let friendship = "b".repeat(64);
    select_friend_receiver(&port, "here", &friendship, Selection::Reset, 10, true)
        .await
        .unwrap();
    let trace = port.trace.lock();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0]["call"], "friendsSetFriendReceiver");
    assert_eq!(trace[0]["command"]["friendship_id"], friendship);
    assert_eq!(trace[0]["command"]["operation"], "reset");
    assert_eq!(trace[0]["command"]["expected_revision"], 10);
}

#[tokio::test]
async fn failed_installation_enrollment_never_submits_a_friend_receiver_mutation() {
    let port = Port {
        account_installation: Some("elsewhere"),
        fail_enrollment: true,
        trace: Default::default(),
    };
    assert_eq!(
        select_friend_receiver(&port, "here", &"a".repeat(64), Selection::Set, 9, true).await,
        Err(Error::KeyLimit {
            limit: 5,
            active: 5
        })
    );
    let trace = port.trace.lock();
    assert_eq!(trace.len(), 2);
    assert_eq!(
        trace[1],
        json!({"call":"enroll","installation_only":true,"takeover":false,"revision":3})
    );
    assert!(!trace
        .iter()
        .any(|call| call["call"] == "friendsSetFriendReceiver"));
}

#[test]
fn stale_revision_and_unconfirmed_takeover_are_refused_before_mutation() {
    let mut status = ReceiverStatus {
        receiver_revision: 4,
        receiver_installation_id: Some("other".into()),
        receiver_machine_label: Some("Anya's Mac".into()),
        receiver_key_id: None,
        receiving_enabled: true,
        is_me: false,
        legacy_active: false,
    };
    assert_eq!(
        validate_selection(&status, 3, true),
        Err(Error::ReceiverRevisionStale)
    );
    assert_eq!(
        validate_selection(&status, 4, false),
        Err(Error::ReceiverElsewhere)
    );
    assert_eq!(validate_selection(&status, 4, true), Ok(()));
    status.receiver_installation_id = None;
    status.receiving_enabled = false;
    assert_eq!(validate_selection(&status, 4, false), Ok(()));
}

#[tokio::test]
async fn first_claim_sends_no_takeover_and_only_the_named_friend() {
    let port = Port {
        account_installation: None,
        fail_enrollment: false,
        trace: Default::default(),
    };
    select_friend_receiver(&port, "here", &"c".repeat(64), Selection::Set, 0, false)
        .await
        .unwrap();
    let trace = port.trace.lock();
    assert_eq!(trace[2]["command"]["friendship_id"], "c".repeat(64));
    assert_eq!(trace[2]["command"]["takeover"], false);
    assert_eq!(trace[2]["command"]["expected_revision"], 0);
}
