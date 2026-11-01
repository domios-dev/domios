use super::*;

fn status(here: bool, revision: u64) -> FriendReceiver {
    FriendReceiver::from_status(&ReceiverStatus {
        receiver_revision: revision,
        receiver_installation_id: Some(if here { "here" } else { "elsewhere" }.into()),
        receiver_encryption_key_id: Some(if here { "vk_here" } else { "vk_elsewhere" }.into()),
        receiver_machine_label: Some("Desk".into()),
        receiving_enabled: true,
        is_me: here,
    })
}

#[test]
fn friendship_admission_requires_known_enabled_current_generation_and_exact_revision() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    assert!(!service.neighbor_friend_receiving_here("friend"));
    service.set_neighbor_friend_receiver_for_test("friend", Some(true), 7);
    assert!(service.neighbor_friend_receiving_here("friend"));
    assert!(service
        .require_neighbor_friend_receiver("friend", Some(7))
        .is_ok());
    assert_eq!(
        service.require_neighbor_friend_receiver("friend", Some(6)),
        Err(Error::NotReceiver)
    );
    assert!(!service.neighbor_friend_receiving_here("unknown"));
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = false;
    assert!(!service.neighbor_friend_receiving_here("friend"));
    service
        .ext::<crate::config::ConfigState>()
        .lock()
        .neighbors
        .enabled = true;
    service
        .neighbor_cell()
        .generation
        .fetch_add(1, Ordering::AcqRel);
    assert!(!service.neighbor_friend_receiving_here("friend"));
    service.set_neighbor_friend_receiver_for_test("friend", Some(false), 8);
    assert!(!service.neighbor_friend_receiving_here("friend"));
    service.set_neighbor_friend_receiver_for_test("friend", None, 8);
    assert!(service.neighbor_friend_receiver("friend").is_none());
}

#[test]
fn moving_one_friend_changes_only_its_authority_and_duplicate_snapshots_are_inert() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let generation = service.neighbor_cell().generation.load(Ordering::Acquire);
    let initial = BTreeMap::from([
        ("first".into(), status(true, 1)),
        ("second".into(), status(true, 1)),
    ]);
    assert_eq!(
        service
            .remember_neighbor_friend_receivers(initial.clone(), generation)
            .unwrap()
            .len(),
        2
    );
    assert!(service
        .remember_neighbor_friend_receivers(initial, generation)
        .unwrap()
        .is_empty());
    let moved = BTreeMap::from([
        ("first".into(), status(false, 2)),
        ("second".into(), status(true, 1)),
    ]);
    let changes = service
        .remember_neighbor_friend_receivers(moved.clone(), generation)
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].friendship_id, "first");
    assert!(changes[0]
        .previous
        .as_ref()
        .unwrap()
        .status
        .receiving_here());
    assert!(!changes[0].current.as_ref().unwrap().status.receiving_here());
    assert!(!service.neighbor_friend_receiving_here("first"));
    assert!(service.neighbor_friend_receiving_here("second"));
    assert!(service
        .require_neighbor_friend_receiver("second", Some(1))
        .is_ok());
    assert!(service
        .remember_neighbor_friend_receivers(moved, generation)
        .unwrap()
        .is_empty());
    let changes = service
        .remember_neighbor_friend_receivers(
            BTreeMap::from([("second".into(), status(true, 1))]),
            generation,
        )
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].friendship_id, "first");
    assert!(changes[0].current.is_none());
    assert!(service.neighbor_friend_receiving_here("second"));
}

#[test]
fn an_old_account_snapshot_cannot_authorize_a_new_session() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let generation = service.neighbor_cell().generation.load(Ordering::Acquire);
    service.set_neighbor_friend_receiver_for_test("friend", Some(true), 1);
    service
        .neighbor_cell()
        .generation
        .fetch_add(1, Ordering::AcqRel);
    assert!(service
        .remember_neighbor_friend_receivers(
            BTreeMap::from([("friend".into(), status(true, 1))]),
            generation
        )
        .is_err());
    assert!(!service.neighbor_friend_receiving_here("friend"));
    service.forget_neighbor_friend_receivers();
    assert!(service.neighbor_friend_receiver("friend").is_none());
}

#[test]
fn binding_digest_ignores_labels_but_detects_equal_revision_key_changes() {
    let projection = |label: &str, key: &str| codeterm_neighbors::wire::EffectiveReceiver {
        installation_id: Some("here".into()),
        enc_key_id: Some(key.into()),
        machine_label: Some(label.into()),
        revision: 2,
        receiving: true,
    };
    let first = FriendReceiver::from_projection(projection("Desk", "session-one"), "here").unwrap();
    let renamed =
        FriendReceiver::from_projection(projection("Renamed", "session-one"), "here").unwrap();
    let next = FriendReceiver::from_projection(projection("Desk", "session-two"), "here").unwrap();
    assert_eq!(first.binding, renamed.binding);
    assert_ne!(first.binding, next.binding);
}

#[tokio::test]
async fn deferred_publication_keeps_its_batch_version_after_invalidation() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let generation = service.neighbor_cell().generation.load(Ordering::Acquire);
    let changes = service
        .remember_neighbor_friend_receivers(
            BTreeMap::from([
                ("first".into(), status(true, 1)),
                ("second".into(), status(true, 1)),
            ]),
            generation,
        )
        .unwrap();
    let old_version = changes[0].version;
    assert!(changes.iter().all(|change| change.version == old_version));
    let (ready, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let publisher = service.clone();
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let publication = tokio::spawn(async move {
        ready.send(()).unwrap();
        released.await.unwrap();
        publisher
            .publish_neighbor_friend_receiver_changes(&metadata, "owner", changes, generation)
            .await
    });
    waiting.await.unwrap();
    let invalidated_version = service.forget_neighbor_friend_receivers();
    service.ui.emit(
        "neighbor-runtime-status",
        json!({"receiver_unavailable":true,"friend_receivers_version":invalidated_version}),
    );
    release.send(()).unwrap();
    publication.await.unwrap().unwrap();
    assert!(invalidated_version > old_version);
    let events = fixture.ui.0.lock();
    let observations: Vec<_> = events
        .iter()
        .filter(|(name, payload)| {
            name == "neighbor-runtime-status" && payload.get("friend_receivers_version").is_some()
        })
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(observations.len(), 3);
    assert_eq!(
        observations[0]["friend_receivers_version"],
        invalidated_version
    );
    for payload in &observations[1..] {
        assert_eq!(payload["friend_receivers_version"], old_version);
        assert_eq!(payload["receiving_here"], true);
    }
    let (snapshot, version) = service.neighbor_friend_receiver_snapshot();
    assert_eq!(snapshot, json!({}));
    assert_eq!(version, invalidated_version);
    let next_invalidation = service.forget_neighbor_friend_receiver("first");
    assert!(next_invalidation > invalidated_version);
}

struct NoCalls;
#[async_trait::async_trait]
impl codeterm_neighbors::adapter::AuthenticatedCall for NoCalls {
    async fn invoke(&self, _: &str, _: Value) -> codeterm_neighbors::Result<Value> {
        panic!("no network in receiver model fixture")
    }
    async fn id_token(&self) -> codeterm_neighbors::Result<Zeroizing<String>> {
        panic!("no authentication in receiver model fixture")
    }
}

#[tokio::test]
async fn receiver_rows_are_per_friend_and_missing_projections_never_inherit_authority() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let metadata = Metadata::new(fixture.temp.path().to_owned(), "owner");
    let registry = FirebaseRegistry {
        calls: Arc::new(NoCalls),
        uid: "owner".into(),
        installation_id: "here".into(),
        machine_label: "Desk".into(),
    };
    let generation = service.neighbor_history_generation();
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let projection = |here: bool, revision| {
        json!({"installation_id":if here {"here"} else {"elsewhere"},
        "enc_key_id":"ek_receiver","machine_label":"Desk","revision":revision,"receiving":true})
    };
    let list = json!({"friends":[{"friendship_id":first,"my_receiver":projection(false,9)},{"friendship_id":second,"my_receiver":projection(true,2)}]});
    service
        .reconcile_neighbor_friend_receiver_rows(
            &metadata,
            &registry,
            &list,
            Some("ek_receiver"),
            generation,
        )
        .await
        .unwrap();
    assert!(!service.neighbor_friend_receiving_here(&first));
    assert!(service.neighbor_friend_receiving_here(&second));
    let (_, version) = service.neighbor_friend_receiver_snapshot();
    service
        .reconcile_neighbor_friend_receiver_rows(
            &metadata,
            &registry,
            &list,
            Some("ek_receiver"),
            generation,
        )
        .await
        .unwrap();
    assert_eq!(service.neighbor_friend_receiver_snapshot().1, version);
    assert!(service
        .reconcile_neighbor_friend_receiver_rows(
            &metadata,
            &registry,
            &json!({"friends":[{"friendship_id":first}]}),
            None,
            generation
        )
        .await
        .is_err());
}

#[tokio::test]
async fn targeted_listener_stop_keeps_other_friends_and_cached_registry_rows() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let service = &fixture.service;
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    service.remember_neighbor_registry(&json!({"access":"paid","friends":[{"friendship_id":first,"displayName":"First"},{"friendship_id":second,"displayName":"Second"}]}));
    let add = |friendship: &str| {
        let (shutdown, mut stopped) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            let _ = stopped.changed().await;
        });
        service.neighbor_cell().listeners.lock().active.insert(
            friendship.into(),
            listener::Handle {
                gateway_tab: None,
                task,
                retry: Arc::new(tokio::sync::Notify::new()),
                shutdown: shutdown.clone(),
            },
        );
        shutdown
    };
    let first_stop = add(&first);
    let second_stop = add(&second);
    service.stop_neighbor_concierge_listener(&first);
    service.stop_neighbor_concierge_listener(&first);
    assert!(*first_stop.borrow());
    assert!(!*second_stop.borrow());
    assert!(service.neighbor_registry_receiving(&first));
    assert!(service.neighbor_registry_receiving(&second));
    let retired = service
        .neighbor_cell()
        .listeners
        .lock()
        .retired
        .remove(&first)
        .unwrap();
    retired.await.unwrap();
    let other = service
        .neighbor_cell()
        .listeners
        .lock()
        .active
        .remove(&second)
        .unwrap();
    other.task.abort();
    let _ = other.task.await;
}
