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

#[tokio::test]
async fn renamed_registration_refreshes_the_same_key_once_before_selecting_only_the_requested_friend(
) {
    use codeterm_neighbors::adapter::AuthenticatedCall;
    use codeterm_neighbors::crypto::PrivateKeys;
    use rand::SeedableRng;
    struct Calls {
        trace: parking_lot::Mutex<Vec<(String, Value)>>,
        key_id: String,
    }
    #[async_trait::async_trait]
    impl AuthenticatedCall for Calls {
        async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
            self.trace.lock().push((route.into(), data));
            match route {
                "friendsEnrollVerifierKey" => {
                    Ok(json!({"verifier_key_id":self.key_id,"revoked_key_ids":[]}))
                }
                "friendsSetFriendReceiver" => Ok(json!({"selected":true})),
                _ => panic!("registration must not revoke or rotate keys"),
            }
        }
        async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
            panic!("no authentication")
        }
    }
    struct BindingPort {
        registry: FirebaseRegistry,
        keys: PrivateKeys,
        registered: tokio::sync::Mutex<Option<RegistrationBinding>>,
    }
    #[async_trait::async_trait]
    impl SelectionPort for BindingPort {
        async fn enroll(&self) -> codeterm_neighbors::Result<()> {
            let binding =
                RegistrationBinding::new(&self.registry, 7, 11, self.keys.verifier_key_id("owner"));
            refresh_registration(
                &mut *self.registered.lock().await,
                binding,
                &self.registry,
                &self.keys,
            )
            .await
        }
        async fn select(&self, command: Value) -> codeterm_neighbors::Result<Value> {
            self.registry
                .calls
                .invoke("friendsSetFriendReceiver", command)
                .await
        }
    }
    let keys = PrivateKeys::generate(&mut rand::rngs::StdRng::seed_from_u64(31));
    let calls = Arc::new(Calls {
        trace: Default::default(),
        key_id: keys.verifier_key_id("owner"),
    });
    let mut port = BindingPort {
        registry: FirebaseRegistry {
            calls: calls.clone(),
            uid: "owner".into(),
            installation_id: "here".into(),
            machine_label: "Studio".into(),
        },
        keys,
        registered: Default::default(),
    };
    select_friend_receiver(&port, "here", "first", Selection::Set, 0, false)
        .await
        .unwrap();
    select_friend_receiver(&port, "here", "second", Selection::Set, 0, false)
        .await
        .unwrap();
    port.registry.machine_label = "Laptop".into();
    select_friend_receiver(&port, "here", "third", Selection::Set, 0, false)
        .await
        .unwrap();
    select_friend_receiver(&port, "here", "fourth", Selection::Set, 0, false)
        .await
        .unwrap();
    let trace = calls.trace.lock();
    let enrollments: Vec<_> = trace
        .iter()
        .filter(|(route, _)| route == "friendsEnrollVerifierKey")
        .collect();
    assert_eq!(enrollments.len(), 2);
    assert_eq!(enrollments[0].1["machine_label"], "Studio");
    assert_eq!(enrollments[1].1["machine_label"], "Laptop");
    assert_eq!(
        enrollments[0].1["public_key_spki_b64"],
        enrollments[1].1["public_key_spki_b64"]
    );
    assert_eq!(
        enrollments[0].1["encryption_public_key_spki_b64"],
        enrollments[1].1["encryption_public_key_spki_b64"]
    );
    assert_eq!(trace[3].0, "friendsEnrollVerifierKey");
    assert_eq!(trace[4].1["friendship_id"], "third");
    assert_eq!(
        trace
            .iter()
            .filter(|(route, _)| route == "friendsSetFriendReceiver")
            .map(|(_, data)| data["friendship_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["first", "second", "third", "fourth"]
    );
    let binding = RegistrationBinding::new(&port.registry, 7, 11, calls.key_id.clone());
    for changed in 0..4 {
        let mut next = binding.clone();
        match changed {
            0 => next.uid = "other-owner".into(),
            1 => next.installation_id = "other-machine".into(),
            2 => next.login_generation += 1,
            _ => next.generation += 1,
        }
        assert!(next != binding);
    }
}
