use super::*;

fn authority() -> UserSendAuthority {
    UserSendAuthority {
        tab_id: "opaque-tab".into(),
        pty_id: Some("opaque-pty".into()),
        binding_nonce: "opaque-nonce".into(),
        pty_session_generation: Some(1),
        provider_id: "opaque-provider".into(),
        session_id: Some("opaque-session".into()),
        observed_session_id: Some("opaque-session".into()),
        prelaunch: false,
    }
}

fn reserve(store: &UserSendStore, sequence: u64) -> UserReservation {
    store
        .reserve(authority(), Some(format!("token-{sequence}")), || {
            (uuid::Uuid::new_v4().to_string(), sequence)
        })
        .unwrap()
}

fn admit(store: &UserSendStore, sequence: u64, text: &str) -> UserSendRecord {
    let ticket = reserve(store, sequence);
    store.admit(&ticket, text, text, None).unwrap()
}

#[test]
fn reversed_preparation_retains_original_bytes_and_reserved_order_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = reserve(&store, 1);
    let second = reserve(&store, 2);
    let a = " \topaque-a\r\nλ\t ";
    let b = "\topaque-b\n界  ";
    let second_record = store.admit(&second, b, b.trim(), None).unwrap();
    drop(second);
    assert!(!store.eligible(&second_record));
    let first_record = store.admit(&first, a, a.trim(), None).unwrap();
    drop(first);
    assert_eq!(
        store.head("opaque-tab").unwrap().unwrap().send_id,
        first_record.send_id
    );
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let records = reopened.records("opaque-tab").unwrap();
    assert!(records.iter().all(|record| record.held_before_readiness));
    assert_eq!(
        records.iter().map(|r| &r.send_id).collect::<Vec<_>>(),
        vec![&first_record.send_id, &second_record.send_id]
    );
    assert_eq!(
        reopened
            .original("opaque-tab", &first_record.send_id)
            .unwrap(),
        a
    );
    assert_eq!(
        reopened
            .original("opaque-tab", &second_record.send_id)
            .unwrap(),
        b
    );
    assert_eq!(records[0].client_token.as_deref(), Some("token-1"));
    assert!(records[0].acceptance_sequence < records[1].acceptance_sequence);
    assert!(records.iter().all(|record| !reopened.eligible(record)));
}

#[test]
fn equal_text_has_distinct_identity_and_later_send_does_not_wait_for_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&store, 1, "opaque-equal");
    let second = admit(&store, 2, "opaque-equal");
    assert_ne!(first.send_id, second.send_id);
    assert!(store.claim(&second, "opaque-session").unwrap());
    store.finish_handoff(&second, false).unwrap();
    assert!(!store.claim(&first, "other-session").unwrap());
    assert!(store.claim(&first, "opaque-session").unwrap());
    assert!(!store.claim(&first, "opaque-session").unwrap());
    assert!(!store.complete(&first).unwrap());
    store.finish_handoff(&first, false).unwrap();
    assert_eq!(
        store.head("opaque-tab").unwrap().unwrap().state,
        UserSendState::Uncertain
    );
    assert!(!store.claim(&first, "opaque-session").unwrap());
    assert_eq!(
        store.original("opaque-tab", &second.send_id).unwrap(),
        "opaque-equal"
    );
}

#[test]
fn crash_in_claim_gap_and_revoked_owner_never_replay_or_remove_tail() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&store, 1, "opaque-first");
    let second = admit(&store, 2, "opaque-second");
    assert!(store.claim(&first, "opaque-session").unwrap());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let head = reopened.head("opaque-tab").unwrap().unwrap();
    assert_eq!(head.state, UserSendState::Claimed);
    assert!(!reopened.claim(&head, "opaque-session").unwrap());
    assert!(!reopened.complete(&head).unwrap());
    assert_eq!(
        reopened.original("opaque-tab", &first.send_id).unwrap(),
        "opaque-first"
    );
    assert_eq!(
        reopened.original("opaque-tab", &second.send_id).unwrap(),
        "opaque-second"
    );
    assert!(store.claim(&second, "opaque-session").unwrap());
    store.finish_handoff(&second, false).unwrap();
    store
        .tab("opaque-tab")
        .revoked
        .store(true, Ordering::Release);
    store.finish_handoff(&first, false).unwrap();
    assert!(!store.complete(&first).unwrap());
    assert!(store
        .reserve(authority(), None, || (uuid::Uuid::new_v4().to_string(), 3))
        .is_err());
}

#[test]
fn proven_zero_write_returns_send_to_queued_without_settlement_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&store, 1, "opaque-first");
    let second = admit(&store, 2, "opaque-second");
    assert!(store.claim(&first, "opaque-session").unwrap());
    store.finish_handoff(&first, true).unwrap();
    assert_eq!(
        store.head("opaque-tab").unwrap().unwrap().state,
        UserSendState::Queued
    );
    assert!(store.claim(&second, "opaque-session").unwrap());
    store.finish_handoff(&second, false).unwrap();
    assert!(store.claim(&first, "opaque-session").unwrap());
}

#[test]
fn ended_launch_discards_only_never_written_queued_records() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let queued = admit(&store, 1, "opaque-stale");
    let restarted = UserSendStore::open(dir.path()).unwrap();
    let mut records = restarted.records("opaque-tab").unwrap();
    let queued_after_restart = records.remove(0);
    assert_eq!(queued.owner_epoch, queued_after_restart.owner_epoch);
    assert_ne!(queued.owner_epoch, restarted.epoch);
    assert!(restarted
        .discard_queued_from_ended_launch(&queued_after_restart)
        .unwrap());
    assert!(restarted.records("opaque-tab").unwrap().is_empty());
    assert!(restarted.original("opaque-tab", &queued.send_id).is_err());

    let handed = admit(&restarted, 2, "opaque-handed");
    assert!(restarted.claim(&handed, "opaque-session").unwrap());
    restarted.finish_handoff(&handed, false).unwrap();
    let next_owner = UserSendStore::open(dir.path()).unwrap();
    let mut records = next_owner.records("opaque-tab").unwrap();
    let handed_after_restart = records.remove(0);
    assert!(!next_owner
        .discard_queued_from_ended_launch(&handed_after_restart)
        .unwrap());
    assert_eq!(handed_after_restart.state, UserSendState::Uncertain);
    assert_eq!(
        next_owner.original("opaque-tab", &handed.send_id).unwrap(),
        "opaque-handed"
    );
}

#[test]
fn record_capacity_refuses_without_evicting_any_accepted_original() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let records: Vec<_> = (1..=MAX_UNSETTLED)
        .map(|i| admit(&store, i as u64, "opaque-body"))
        .collect();
    assert!(store
        .reserve(authority(), None, || (uuid::Uuid::new_v4().to_string(), 33))
        .is_err());
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert_eq!(reopened.records("opaque-tab").unwrap().len(), MAX_UNSETTLED);
    for record in records {
        assert_eq!(
            reopened.original("opaque-tab", &record.send_id).unwrap(),
            "opaque-body"
        );
    }
}

#[test]
fn payload_and_index_failures_preserve_accepted_addressable_records() {
    for stage in ["payload", "index"] {
        let dir = tempfile::tempdir().unwrap();
        let store = UserSendStore::open(dir.path()).unwrap();
        let first = admit(&store, 1, "opaque-accepted");
        store.set_persistence_boundary(Some(Arc::new(move |observed| {
            if observed == stage {
                Err("opaque-storage-failure".into())
            } else {
                Ok(())
            }
        })));
        let refused = reserve(&store, 2);
        assert!(store
            .admit(&refused, "opaque-refused", "opaque-refused", None)
            .is_err());
        drop(refused);
        let reopened = UserSendStore::open(dir.path()).unwrap();
        assert_eq!(reopened.records("opaque-tab").unwrap().len(), 1);
        assert_eq!(
            reopened.original("opaque-tab", &first.send_id).unwrap(),
            "opaque-accepted"
        );
    }
}

fn body_admission(
    store: &UserSendStore,
    sequence: u64,
    returned: &Path,
    bytes: Vec<u8>,
) -> Result<UserSendRecord, String> {
    let ticket = reserve(store, sequence);
    store.admit(
        &ticket,
        "opaque-original",
        &format!("Read and follow the message in {}", returned.display()),
        Some(PreparedUserBody {
            returned_path: returned.to_path_buf(),
            bytes,
        }),
    )
}

#[test]
fn pointer_body_outlives_old_pad_submission_settlement_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("owned user λ 界");
    #[cfg(windows)]
    let data_dir = (0..4).fold(data_dir, |path, _| {
        path.join("owned-path-component".repeat(4))
    });
    fs::create_dir_all(&data_dir).unwrap();
    let data_dir = data_dir.canonicalize().unwrap();
    let returned = data_dir.join("opaque pad λ 界.md");
    let bytes = b"opaque-prepared\r\nbody  ".to_vec();
    fs::write(&returned, &bytes).unwrap();
    let store = UserSendStore::open(&data_dir).unwrap();
    let record = body_admission(&store, 1, &returned, bytes.clone()).unwrap();
    fs::remove_file(&returned).unwrap();
    let owned = store.body_path(&record).unwrap().unwrap();
    assert_ne!(owned, returned);
    assert!(owned.to_str().unwrap().contains("owned user λ 界"));
    #[cfg(windows)]
    {
        assert!(owned.as_os_str().len() > 260);
        assert!(!store.prepared(&record).unwrap().contains(r"\\?\"));
    }
    assert_eq!(fs::read(&owned).unwrap(), bytes);
    assert!(store
        .prepared(&record)
        .unwrap()
        .ends_with(&codeterm_paths::strip_verbatim_prefix(
            owned.to_str().unwrap()
        )));
    assert!(store.claim(&record, "opaque-session").unwrap());
    store.finish_handoff(&record, false).unwrap();
    assert!(store.complete(&record).unwrap());
    store
        .tab("opaque-tab")
        .revoked
        .store(true, Ordering::Release);
    let reopened = UserSendStore::open(&data_dir).unwrap();
    let retained = reopened.records("opaque-tab").unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].state, UserSendState::BodyLease);
    assert_eq!(
        fs::read(reopened.body_path(&retained[0]).unwrap().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(
        reopened.original("opaque-tab", &record.send_id).unwrap(),
        "opaque-original"
    );
    assert_eq!(
        reopened.head("opaque-tab").unwrap().map(|r| r.send_id),
        None
    );
}

#[test]
fn actual_body_bytes_and_retained_leases_count_toward_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let returned = dir.path().join("opaque.md");
    let record =
        body_admission(&store, 1, &returned, vec![b'x'; MAX_PREPARED_BYTES - 2048]).unwrap();
    assert!(store.claim(&record, "opaque-session").unwrap());
    store.finish_handoff(&record, false).unwrap();
    assert!(store.complete(&record).unwrap());
    assert!(body_admission(&store, 2, &returned, vec![b'y'; 2048]).is_err());
    assert_eq!(store.records("opaque-tab").unwrap().len(), 1);
    assert_eq!(
        fs::metadata(store.body_path(&record).unwrap().unwrap())
            .unwrap()
            .len(),
        (MAX_PREPARED_BYTES - 2048) as u64
    );
}

#[test]
fn unexpected_pointer_shape_and_original_byte_overflow_refuse_without_eviction() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let first = admit(&store, 1, "opaque-retained");
    assert!(store
        .reserve(
            authority(),
            Some("x".repeat(MAX_INDEX_BYTES as usize + 1)),
            || panic!("oversized metadata must refuse before identity reservation")
        )
        .is_err());
    let ticket = reserve(&store, 2);
    assert!(store
        .admit(
            &ticket,
            "opaque-new",
            "opaque-user-supplied-pointer",
            Some(PreparedUserBody {
                returned_path: dir.path().join("opaque.md"),
                bytes: b"opaque-body".to_vec(),
            })
        )
        .is_err());
    drop(ticket);
    let ticket = reserve(&store, 3);
    assert!(store
        .admit(&ticket, &"x".repeat(MAX_PREPARED_BYTES), "opaque", None)
        .is_err());
    assert_eq!(
        store.original("opaque-tab", &first.send_id).unwrap(),
        "opaque-retained"
    );
    assert_eq!(store.records("opaque-tab").unwrap().len(), 1);
}

#[test]
fn bootstrap_reopen_does_not_sweep_user_namespace() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1, "opaque-user");
    let _bootstrap =
        super::super::PendingDeliveryPayloadStore::open(dir.path(), &HashSet::new()).unwrap();
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert_eq!(
        reopened.original("opaque-tab", &record.send_id).unwrap(),
        "opaque-user"
    );
}

#[test]
fn missing_index_with_retained_payload_refuses_without_orphan_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1, "opaque-retained");
    let tab = store.tab("opaque-tab");
    fs::remove_file(tab.dir.join("index.json")).unwrap();
    let path = tab.payload_path(&record.send_id, "original").unwrap();
    let reopened = UserSendStore::open(dir.path()).unwrap();
    assert!(reopened.records("opaque-tab").is_err());
    assert_eq!(fs::read(path).unwrap(), b"opaque-retained");
}

#[cfg(unix)]
#[test]
fn user_namespace_and_payloads_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let record = admit(&store, 1, "opaque");
    let tab = store.tab("opaque-tab");
    assert_eq!(
        fs::metadata(&store.root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&tab.dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(tab.payload_path(&record.send_id, "original").unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn final_transport_and_typed_follow_up_are_frozen_separately_from_original() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let reservation = reserve(&store, 1);
    let final_transport = store
        .final_transport(&reservation, "opaque-transport\0", None)
        .unwrap();
    let record = store
        .admit_with_follow_up(
            &reservation,
            "\topaque-original\u{1b}",
            "opaque-transport\0",
            None,
            Some("opaque-follow-up".into()),
            false,
        )
        .unwrap();
    drop(reservation);
    let reopened = UserSendStore::open(dir.path()).unwrap();
    let records = reopened.records("opaque-tab").unwrap();
    assert!(!records[0].held_before_readiness);
    assert_eq!(
        reopened.original("opaque-tab", &record.send_id).unwrap(),
        "\topaque-original\u{1b}"
    );
    assert_eq!(reopened.prepared(&records[0]).unwrap(), final_transport);
    assert_eq!(final_transport, "opaque-transport");
    assert_eq!(
        records[0].typed_follow_up.as_deref(),
        Some("opaque-follow-up")
    );
}

#[test]
fn body_owner_count_remains_bounded_after_confirmed_pointer_submissions() {
    let dir = tempfile::tempdir().unwrap();
    let store = UserSendStore::open(dir.path()).unwrap();
    let returned = dir.path().join("opaque.md");
    let mut retained = Vec::new();
    for sequence in 1..=MAX_BODY_LEASES as u64 {
        let record = body_admission(&store, sequence, &returned, b"opaque-body".to_vec()).unwrap();
        assert!(store.claim(&record, "opaque-session").unwrap());
        store.finish_handoff(&record, false).unwrap();
        assert!(store.complete(&record).unwrap());
        retained.push(record);
    }
    assert!(body_admission(&store, 33, &returned, b"opaque-refused".to_vec()).is_err());
    assert_eq!(store.records("opaque-tab").unwrap().len(), MAX_BODY_LEASES);
    for record in retained {
        assert_eq!(
            fs::read(store.body_path(&record).unwrap().unwrap()).unwrap(),
            b"opaque-body"
        );
    }
}

#[cfg(windows)]
#[test]
fn junction_owner_is_refused_before_acl_changes_or_external_payload_writes() {
    use base64::Engine;
    let dir = tempfile::tempdir().unwrap();
    let external = dir.path().join("external target λ 界");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("opaque-sentinel"), b"opaque-retained").unwrap();
    let owned = dir.path().join("user-send-payloads");
    let literal = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "''"));
    let script = format!(
        "New-Item -ItemType Junction -Path {} -Target {} -ErrorAction Stop | Out-Null",
        literal(&owned),
        literal(&external)
    );
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut command = std::process::Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(base64::engine::general_purpose::STANDARD.encode(encoded));
    crate::process_util::no_window_std(&mut command);
    assert!(command.status().unwrap().success());
    let before = super::super::acl_setup_calls();
    let store = UserSendStore::open(dir.path()).unwrap();
    let ticket = reserve(&store, 1);
    let error = store
        .admit(&ticket, "opaque-original", "opaque-transport", None)
        .unwrap_err();
    assert!(error.contains("reparse point"));
    assert_eq!(super::super::acl_setup_calls(), before);
    assert_eq!(
        fs::read(external.join("opaque-sentinel")).unwrap(),
        b"opaque-retained"
    );
    assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
    fs::remove_dir(&owned).unwrap();
}
