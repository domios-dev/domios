use super::*;
use crate::multiplexer::state::MultiplexerState;

#[tokio::test]
async fn oversized_sends_are_typed_before_the_network_operation_queue() {
    let (mux, _) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let (owner, mut queued) = tokio::sync::mpsc::channel(1);
    *fixture.service.neighbor_cell().owner.lock() = Some(owner);
    let command = NeighborRuntimeCommand::Send {
        attach: vec![],
        message_id: None,
        friendship_id: "a".repeat(64),
        kind: Kind::Question,
        body: "x".repeat(32753),
        in_reply_to: None,
    };
    tokio::select! {
        result = fixture.service.neighbor_runtime(command, None, None) => assert_eq!(result.unwrap_err(), "message_too_large: {\"limit\":32752}"),
        _ = queued.recv() => panic!("An oversized body cannot reach network work"),
    }
}

#[tokio::test]
async fn local_reads_bypass_blocked_network_owner_and_return_metadata() {
    let (mux, _) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings.permissions.insert(
        friendship.clone(),
        Permission {
            allowed: true,
            scope: "opaque scope".into(),
            allow_tab_requests: false,
            allow_interactive_content: true,
            ..Default::default()
        },
    );
    metadata.save_settings(settings).await.unwrap();
    let (sender, mut requests) = tokio::sync::mpsc::channel(1);
    *service.neighbor_cell().owner.lock() = Some(sender);
    for command in [
        NeighborRuntimeCommand::History {
            friendship_id: friendship.clone(),
            before: None,
            limit: None,
        },
        NeighborRuntimeCommand::Permission {
            friendship_id: friendship.clone(),
        },
        NeighborRuntimeCommand::NotificationPreference {
            friendship_id: friendship.clone(),
        },
        NeighborRuntimeCommand::Status,
    ] {
        assert!(command.is_local_read());
        tokio::select! {
            biased;
            result = service.neighbor_runtime(serde_json::from_value(match &command {
                NeighborRuntimeCommand::History { .. } => json!({"operation":"history","friendship_id":friendship}),
                NeighborRuntimeCommand::Permission { .. } => json!({"operation":"permission","friendship_id":friendship}),
                NeighborRuntimeCommand::NotificationPreference { .. } => json!({"operation":"notification_preference","friendship_id":friendship}),
                _ => json!({"operation":"status"}),
            }).unwrap(), None, None) => assert_eq!(result.unwrap_err(), "Sign in with a hosted account to use Neighbors"),
            _ = requests.recv() => panic!("Local reads cannot enter the blocked network queue"),
        }
        let result = service
            .neighbor_local_metadata(&command, &metadata, service.neighbor_history_generation())
            .await
            .unwrap();
        match command {
            NeighborRuntimeCommand::History { .. } => assert_eq!(result["entries"], json!([])),
            NeighborRuntimeCommand::Permission { .. } => {
                assert_eq!(result["allow_interactive_content"], true)
            }
            NeighborRuntimeCommand::NotificationPreference { .. } => {
                assert_eq!(result["notify"], "in_app")
            }
            _ => {
                assert_eq!(
                    result["permissions"][&friendship]["allow_interactive_content"],
                    true
                );
                assert!(result.get("receiving_here").is_none());
                assert_eq!(result["friend_receivers"], json!({}));
                assert!(result["installation_id"].as_str().is_some());
                assert_eq!(result["projection"]["pending_verifier_ids"], json!([]));
                assert_eq!(result["projection"]["snapshot_version"], Value::Null);
            }
        }
    }
    assert!(requests.try_recv().is_err());
}

#[tokio::test]
async fn status_uses_known_local_receiver_and_real_concierge_lifecycle_while_activation_is_stale() {
    let fixture = crate::test_support::fixture(MultiplexerState::new().0);
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let friendship = "a".repeat(64);
    let mut settings = crate::service::neighbors::storage::Settings::default();
    settings
        .bindings
        .insert(friendship.clone(), "closed-concierge".into());
    metadata.save_settings(settings).await.unwrap();
    service.set_neighbor_friend_receiver_for_test(&friendship, Some(true), 9);
    let generation = service.neighbor_cell().generation.load(Ordering::Acquire);
    service.record_neighbor_activation(generation, Some(Error::Transport.into()));
    let result = service
        .neighbor_local_metadata(&NeighborRuntimeCommand::Status, &metadata, generation)
        .await
        .unwrap();
    assert_eq!(result["status_stale"], true);
    assert_eq!(
        result["friend_receivers"][&friendship]["receiving_here"],
        true
    );
    assert_eq!(result["friend_receivers"][&friendship]["revision"], 9);
    assert_eq!(result["concierges"][&friendship]["receiving_here"], true);
    assert_eq!(
        result["concierges"][&friendship]["state"],
        "concierge_closed"
    );
}

#[tokio::test]
async fn receiver_grants_preview_unassigned_friend_from_current_list_without_installing_authority()
{
    let (mux, tab) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let service = &fixture.service;
    let friendship = "a".repeat(64);
    fixture
        .seed_neighbor_concierge(&tab, &friendship, Permission::default())
        .await;
    service.forget_neighbor_friend_receivers();
    service.neighbor_projection_unavailable();
    let session = service
        .ext::<crate::service::account::AccountCell>()
        .0
        .get()
        .unwrap()
        .session()
        .await
        .unwrap();
    let unknown = "b".repeat(64);
    let list = json!({"access":"paid","friends":[
        {"friendship_id":friendship,"my_receiver":{
            "installation_id":null,"enc_key_id":null,"machine_label":null,"receiving":false,"revision":7
        }},
        {"friendship_id":unknown}
    ]});
    service
        .neighbor_backend_read(
            "friendsList",
            &session.identity.uid,
            session.login_generation,
            async { Ok(list) },
        )
        .await
        .unwrap();
    let result = service
        .neighbor_runtime(
            NeighborRuntimeCommand::ReceiverGrants {
                friendship_id: friendship.clone(),
            },
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result["revision"], 7);
    assert_eq!(result["previous"], Value::Null);
    assert_eq!(result["unavailable"], false);
    assert!(service.neighbor_friend_receiver(&friendship).is_none());
    let error = service
        .neighbor_runtime(
            NeighborRuntimeCommand::ReceiverGrants {
                friendship_id: unknown,
            },
            None,
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error, Error::ReceiverStatusUnknown.to_string());
}

struct TakeoverRegistry {
    uid: String,
    installation: String,
    key: parking_lot::Mutex<Option<(String, String)>>,
    moved: AtomicBool,
}

impl TakeoverRegistry {
    fn list(&self) -> Value {
        let receiver = if self.moved.load(Ordering::Acquire) {
            json!({
                "installation_id":self.installation,"enc_key_id":self.key.lock().as_ref().unwrap().1,
                "machine_label":"This desk","receiving":true,"revision":20
            })
        } else {
            json!({
                "installation_id":"other-desk","enc_key_id":format!("ek_{}", "1".repeat(32)),
                "machine_label":"Other desk","receiving":true,"revision":19
            })
        };
        json!({"access":"paid","friends":[{"friendship_id":"a".repeat(64),"my_receiver":receiver}]})
    }
}

#[async_trait::async_trait]
impl AuthenticatedCall for TakeoverRegistry {
    async fn invoke(&self, route: &str, data: Value) -> codeterm_neighbors::Result<Value> {
        match route {
            "friendsList" => Ok(self.list()),
            "friendsOwnKeys" => Ok(json!(self
                .key
                .lock()
                .iter()
                .map(|(verifier, _)| json!({"verifier_key_id":verifier}))
                .collect::<Vec<_>>())),
            "friendsEnrollVerifierKey" => {
                assert_eq!(data["installation_id"], self.installation);
                let signing = data["public_key_spki_b64"].as_str().unwrap();
                let encryption = data["encryption_public_key_spki_b64"].as_str().unwrap();
                let hash = codeterm_neighbors::crypto::sha256_hex(
                    format!("verifier-key:{}:{signing}", self.uid).as_bytes(),
                );
                let key_id = format!("vk_{}", &hash[..32]);
                let key = (
                    key_id.clone(),
                    codeterm_neighbors::crypto::encryption_key_id(encryption),
                );
                assert!(self.key.lock().replace(key).is_none());
                Ok(json!({"verifier_key_id":key_id}))
            }
            "friendsSetFriendReceiver" => {
                assert_eq!(data["installation_id"], self.installation);
                assert_eq!(data["friendship_id"], "a".repeat(64));
                assert_eq!(data["operation"], "set");
                assert_eq!(data["takeover"], true);
                assert!(self.key.lock().is_some());
                if data["expected_revision"] != 19 || self.moved.swap(true, Ordering::AcqRel) {
                    return Err(Error::ReceiverRevisionStale);
                }
                Ok(json!({"schema_version":1,"receiver":{
                    "friendship_id":data["friendship_id"],"machine_label":"This desk",
                    "revision":20,"receiving":true,"is_me":true
                }}))
            }
            _ => panic!("Unexpected takeover registry call: {route}"),
        }
    }

    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("Receiver selection does not start listeners before commit")
    }
}

#[tokio::test]
async fn receiver_grants_preview_remote_holder_and_commit_checked_cold_start_takeover() {
    use codeterm_neighbors::keys::Slot;

    let (mux, tab) = MultiplexerState::new();
    let fixture = crate::test_support::fixture(mux);
    let service = &fixture.service;
    let friendship = "a".repeat(64);
    fixture
        .seed_neighbor_concierge(&tab, &friendship, Permission::default())
        .await;
    service.forget_neighbor_friend_receivers();
    service.neighbor_projection_unavailable();
    let session = service.account().await.unwrap().session().await.unwrap();
    let uid = &session.identity.uid;
    let installation = service.installation_id().await.unwrap();
    let calls = Arc::new(TakeoverRegistry {
        uid: uid.clone(),
        installation: installation.as_str().into(),
        key: Default::default(),
        moved: AtomicBool::new(false),
    });
    let store = service.fixture_neighbor_key_store();
    let ports = Arc::new(FixtureKeyPorts {
        registry: Arc::new(FirebaseRegistry {
            calls: calls.clone(),
            uid: uid.clone(),
            installation_id: installation.as_str().into(),
            machine_label: "This desk".into(),
        }),
        scope: KeyScope::installation(installation.as_str(), uid).unwrap(),
        metadata: Metadata::new(service.account_data_root().unwrap(), uid),
    });
    service
        .extensions
        .get_or_init(|| FixtureKeyPortBinding(Arc::downgrade(&ports)));
    service
        .neighbor_backend_read("friendsList", uid, session.login_generation, async {
            Ok(calls.list())
        })
        .await
        .unwrap();

    let preview = service
        .neighbor_runtime(
            NeighborRuntimeCommand::ReceiverGrants {
                friendship_id: friendship.clone(),
            },
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(preview["previous_machine"], "Other desk");
    assert_eq!(preview["revision"], 19);
    assert_eq!(preview["unavailable"], true);
    let digest = preview["digest"].as_str().unwrap();
    assert!(hex_id(digest, 64));
    assert!(service.neighbor_friend_receiver(&friendship).is_none());
    assert!(calls.key.lock().is_none());
    assert!(store
        .load(&ports.scope, Slot::Current)
        .await
        .unwrap()
        .is_none());

    let select = |revision, confirmed, digest: &str| {
        let command = serde_json::from_value(json!({
            "operation":"set_friend_receiver","friendship_id":friendship,"receiver_operation":"set",
            "expected_receiver_revision":revision,"takeover_confirmed":confirmed,
            "grant_decision":{"choice":"keep","expected_digest":digest}
        }))
        .unwrap();
        service.neighbor_operation(owner::Operation::Command {
            command,
            caller: None,
            nonce: None,
        })
    };
    for (revision, confirmed, digest, code) in [
        (19, false, digest, "receiving_elsewhere:"),
        (18, true, digest, "receiver_revision_stale:"),
        (19, true, &"0".repeat(64), "receiver_grant_stale:"),
    ] {
        match select(revision, confirmed, digest).await {
            Err(error) => assert!(error.starts_with(code), "{error}"),
            Ok(_) => panic!("Takeover committed without matching consent, revision and digest"),
        }
        assert!(calls.key.lock().is_none());
        assert!(!calls.moved.load(Ordering::Acquire));
    }

    let Ok(owner::Outcome::ReceiverCommitted(phase)) = select(19, true, digest).await else {
        panic!("Confirmed takeover did not commit");
    };
    assert_eq!(phase.committed.result["receiver"]["revision"], 20);
    assert_eq!(phase.committed.result["receiver"]["receiving"], true);
    assert_eq!(phase.committed.result["receiver"]["is_me"], true);
    assert!(store
        .load(&ports.scope, Slot::Current)
        .await
        .unwrap()
        .is_some());
    let current =
        first_action::friend_status(&calls.list(), &friendship, installation.as_str()).unwrap();
    assert!(current.receiving_here());
    assert_eq!(current.receiver_revision, 20);
}

#[tokio::test]
async fn flag_message_persists_without_entering_the_network_operation_queue() {
    let (mut mux, concierge) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&concierge).unwrap();
    tab.agent_type = Some("fixture".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    let fixture = crate::test_support::fixture(mux);
    let friend = "a".repeat(64);
    let message = "1".repeat(32);
    fixture
        .seed_neighbor_concierge(&concierge, &friend, Permission::default())
        .await;
    let service = &fixture.service;
    let account = service
        .ext::<crate::service::account::AccountCell>()
        .0
        .get()
        .unwrap();
    let session = account.session().await.unwrap();
    let metadata = Metadata::new(service.account_data_root().unwrap(), &session.identity.uid);
    let entry = serde_json::from_value(json!({"direction":"in","author":"friend","sender_uid":"peer","body":"opaque private message","at":1,"message_id":message,"kind":"question"})).unwrap();
    service
        .append_neighbor_history(
            &metadata,
            &friend,
            entry,
            service.neighbor_history_generation(),
        )
        .await
        .unwrap();
    let (sender, mut requests) = tokio::sync::mpsc::channel(1);
    *service.neighbor_cell().owner.lock() = Some(sender);
    let command = NeighborRuntimeCommand::FlagMessage {
        friendship_id: friend.clone(),
        message_id: message,
        reason: "Unexpected instructions".into(),
        category: super::super::super::FlagCategory::SuspiciousInstructions,
    };
    tokio::select! {
        result = service.neighbor_runtime(command,Some(&concierge),Some("nonce")) => assert_eq!(result.unwrap()["created"],true),
        _ = requests.recv() => panic!("Flagging cannot enter the network send queue"),
    }
    assert_eq!(
        metadata.history_page(&friend, None, 50).await.unwrap()["entries"][0]["flags"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(requests.try_recv().is_err());
}
