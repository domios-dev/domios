use super::*;

struct Port {
    account_installation: &'static str,
    fail_enrollment: bool,
    trace: parking_lot::Mutex<Vec<Value>>,
}

#[async_trait::async_trait]
impl SelectionPort for Port {
    async fn account_receiver(&self) -> codeterm_neighbors::Result<ReceiverStatus> {
        self.trace.lock().push(json!({"call":"account_status"}));
        Ok(ReceiverStatus {
            receiver_revision: 3,
            receiver_installation_id: Some(self.account_installation.into()),
            receiver_key_id: Some("account-key".into()),
            receiver_machine_label: Some("Account machine".into()),
            receiving_enabled: true,
            is_me: self.account_installation == "here",
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
            account_installation,
            fail_enrollment: false,
            trace: Default::default(),
        };
        assert_eq!(
            select_friend_receiver(&port, "here", &friendship, Selection::Set, 9)
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
        account_installation: "elsewhere",
        fail_enrollment: true,
        trace: Default::default(),
    };
    let friendship = "b".repeat(64);
    select_friend_receiver(&port, "here", &friendship, Selection::Reset, 10)
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
        account_installation: "elsewhere",
        fail_enrollment: true,
        trace: Default::default(),
    };
    assert_eq!(
        select_friend_receiver(&port, "here", &"a".repeat(64), Selection::Set, 9).await,
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
