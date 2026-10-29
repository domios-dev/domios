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

    async fn automatic_snapshot(&self) -> codeterm_neighbors::Result<(ReceiverStatus, Value)> {
        Err(Error::InvalidRequest)
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
            select_friend_receiver(&port, "here", &friendship, Selection::Set, 9, true, false)
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
    select_friend_receiver(
        &port,
        "here",
        &friendship,
        Selection::Reset,
        10,
        true,
        false,
    )
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
        select_friend_receiver(
            &port,
            "here",
            &"a".repeat(64),
            Selection::Set,
            9,
            true,
            false
        )
        .await,
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
    select_friend_receiver(
        &port,
        "here",
        &"c".repeat(64),
        Selection::Set,
        0,
        false,
        false,
    )
    .await
    .unwrap();
    let trace = port.trace.lock();
    assert_eq!(trace[2]["command"]["friendship_id"], "c".repeat(64));
    assert_eq!(trace[2]["command"]["takeover"], false);
    assert_eq!(trace[2]["command"]["expected_revision"], 0);
}

struct AtomicState {
    account: ReceiverStatus,
    friends: Value,
    gap: Option<&'static str>,
    enrollments: usize,
    selections: usize,
    claimed: usize,
}

struct AtomicPort(parking_lot::Mutex<AtomicState>);

fn status(installation: Option<&str>) -> ReceiverStatus {
    ReceiverStatus {
        receiver_revision: 0,
        receiver_installation_id: installation.map(str::to_owned),
        receiver_key_id: None,
        receiver_machine_label: installation.map(str::to_owned),
        receiving_enabled: false,
        is_me: installation == Some("here"),
        legacy_active: false,
    }
}

fn automatic_port(capability: bool, gap: Option<&'static str>) -> AtomicPort {
    AtomicPort(parking_lot::Mutex::new(AtomicState {
        account: status(None),
        friends: json!({"receiver_claim_capabilities":if capability { vec!["account_all_none_v1"] } else { vec![] },
            "friends":[{"friendship_id":"one"},{"friendship_id":"two"}]}),
        gap,
        enrollments: 0,
        selections: 0,
        claimed: 0,
    }))
}

#[async_trait::async_trait]
impl SelectionPort for AtomicPort {
    async fn automatic_snapshot(&self) -> codeterm_neighbors::Result<(ReceiverStatus, Value)> {
        let state = self.0.lock();
        Ok((state.account.clone(), state.friends.clone()))
    }

    async fn account_receiver(&self) -> codeterm_neighbors::Result<ReceiverStatus> {
        Ok(self.0.lock().account.clone())
    }

    async fn enroll(
        &self,
        enrollment: codeterm_neighbors::adapter::Enrollment,
    ) -> codeterm_neighbors::Result<()> {
        assert!(enrollment.installation_only);
        assert!(!enrollment.takeover);
        let mut state = self.0.lock();
        state.enrollments += 1;
        match state.gap.take() {
            Some("account") => state.account = status(Some("foreign")),
            Some("other_friend") => {
                state.friends["friends"][1]["my_receiver"] = json!({
                    "source":"friend","installation_id":"foreign","key_id":null,"enc_key_id":null,
                    "session_id":null,"machine_label":"Foreign Mac","revision":0,"features":[],"receiving":false
                })
            }
            _ => {}
        }
        Ok(())
    }

    async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value> {
        let mut state = self.0.lock();
        state.selections += 1;
        assert_eq!(command["require_no_receivers"], true);
        assert_eq!(command["takeover"], false);
        let id = command["friendship_id"].as_str().unwrap();
        if super::super::super::first_action::first_claim_revision(
            &state.friends,
            id,
            &state.account,
            "here",
        )? != Some(0)
        {
            return Err(Error::ReceiversPresent);
        }
        let friend = state.friends["friends"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|friend| friend["friendship_id"] == id)
            .unwrap();
        friend["my_receiver"] = json!({
            "source":"friend","installation_id":"here","key_id":null,"enc_key_id":null,
            "session_id":null,"machine_label":"Here","revision":1,"features":[],"receiving":false
        });
        state.claimed += 1;
        Ok(json!({"selected":true}))
    }
}

#[tokio::test]
async fn two_first_actions_claim_only_the_first_friend_from_fresh_admission() {
    let port = automatic_port(true, None);
    select_friend_receiver(&port, "here", "one", Selection::Set, 0, false, true)
        .await
        .unwrap();
    assert_eq!(
        select_friend_receiver(&port, "here", "two", Selection::Set, 0, false, true).await,
        Err(Error::ReceiverNotChosen)
    );
    let state = port.0.lock();
    assert_eq!(
        (state.enrollments, state.selections, state.claimed),
        (1, 1, 1)
    );
}

#[tokio::test]
async fn atomic_all_none_precondition_refuses_an_intervening_account_or_other_friend_receiver() {
    for gap in ["account", "other_friend"] {
        let port = automatic_port(true, Some(gap));
        assert_eq!(
            select_friend_receiver(&port, "here", "one", Selection::Set, 0, false, true).await,
            Err(Error::ReceiversPresent)
        );
        assert_eq!(port.0.lock().claimed, 0);
        assert_eq!(
            select_friend_receiver(&port, "here", "one", Selection::Set, 0, false, true).await,
            Err(Error::ReceiverNotChosen)
        );
        let state = port.0.lock();
        assert_eq!((state.enrollments, state.selections), (1, 1));
    }
}

#[tokio::test]
async fn absent_global_claim_capability_performs_no_enrollment_or_selection() {
    for capability in [
        Value::Null,
        json!(["unknown"]),
        json!({}),
        json!([true]),
        json!(["account_all_none_v1", true]),
    ] {
        let port = automatic_port(false, None);
        port.0.lock().friends["receiver_claim_capabilities"] = capability;
        assert_eq!(
            select_friend_receiver(&port, "here", "one", Selection::Set, 0, false, true).await,
            Err(Error::ReceiverNotChosen)
        );
        let state = port.0.lock();
        assert_eq!(
            (state.enrollments, state.selections, state.claimed),
            (0, 0, 0)
        );
    }
    let port = automatic_port(false, None);
    port.0
        .lock()
        .friends
        .as_object_mut()
        .unwrap()
        .remove("receiver_claim_capabilities");
    assert_eq!(
        select_friend_receiver(&port, "here", "one", Selection::Set, 0, false, true).await,
        Err(Error::ReceiverNotChosen)
    );
    let state = port.0.lock();
    assert_eq!(
        (state.enrollments, state.selections, state.claimed),
        (0, 0, 0)
    );
}
