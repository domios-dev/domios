use super::*;

fn owners(complete: bool) -> RestoredPayloadOwners {
    RestoredPayloadOwners {
        live: HashSet::from(["live".into()]),
        closed: HashSet::from(["closed".into(), "live".into()]),
        complete,
    }
}

#[test]
fn live_owner_always_retains_payloads() {
    for complete in [false, true] {
        assert!(!owners(complete).should_remove("live"));
    }
}

#[test]
fn closed_owner_removes_payloads() {
    for complete in [false, true] {
        assert!(owners(complete).should_remove("closed"));
    }
}

#[test]
fn unknown_owner_requires_complete_restore() {
    assert!(!owners(false).should_remove("unknown"));
    assert!(owners(true).should_remove("unknown"));
}

#[test]
fn persisted_tab_not_yet_restored_retains_payloads() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let persisted = mux.to_session_snapshot();
    let mut partial_restore = mux.snapshot();
    partial_restore.tabs.remove(&tab_id);
    let owners = RestoredPayloadOwners::new(&partial_restore, Some(&persisted));
    assert!(!owners.should_remove(&digest(tab_id.as_bytes())));
    assert!(owners.should_remove(&digest(b"absent-from-persistence")));
}

#[test]
fn retention_wiring_preserves_live_and_purges_closed_and_startup_orphans() {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;

    let (mux, live_id) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let store = &fixture.service.pending_delivery_payloads.user_sends;
    let live = store.tab(&live_id);
    let closed = store.tab("closed-tab");
    let orphan = store.tab("orphan-tab");
    for tab in [&live, &closed, &orphan] {
        fs::create_dir_all(&tab.dir).unwrap();
        fs::write(tab.dir.join("plaintext"), b"opaque retained payload").unwrap();
    }

    fixture.service.finish_tab_close(&live_id).unwrap();
    assert!(live.dir.join("plaintext").exists());
    fixture.service.finish_tab_close("closed-tab").unwrap();
    assert!(!closed.dir.exists());
    assert!(store.load(&closed, &mut closed.data.lock()).is_err());
    assert!(!closed.dir.exists());

    let restarted = UserSendStore::open(store.root.parent().unwrap()).unwrap();
    let snapshot = fixture.service.mux.read().snapshot();
    let persisted = fixture.service.mux.read().to_session_snapshot();
    restarted
        .sweep_closed_tabs(&RestoredPayloadOwners::new(&snapshot, Some(&persisted)))
        .unwrap();
    assert!(live.dir.join("plaintext").exists());
    assert!(!orphan.dir.exists());
}
