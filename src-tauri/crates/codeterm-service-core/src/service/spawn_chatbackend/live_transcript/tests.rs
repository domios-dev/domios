use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::live_transcript::{CursorStart, DEFAULT_RAW_BUDGET_BYTES};
use crate::service::test_fixture::ServiceFixture;
use crate::service::unattended_launch::{self, LaunchPurpose, UnattendedLaunch};

#[test]
fn transcript_router_purpose_refuses_a_provider_without_a_local_runtime() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "chat",
    )));
    let launch = UnattendedLaunch {
        provider_id: "chat",
        purpose: LaunchPurpose::TranscriptRouter,
        ..Default::default()
    };
    assert!(unattended_launch::preflight(&registry, &launch).is_err());
    assert!(unattended_launch::grant_args(&registry, &launch).is_empty());
    let agent_launch = UnattendedLaunch {
        purpose: LaunchPurpose::Agent,
        ..launch
    };
    assert!(unattended_launch::preflight(&registry, &agent_launch).is_ok());
}

fn epoch(runtime_tab_id: String) -> ConciergeEpoch {
    ConciergeEpoch {
        epoch_id: "epoch".into(),
        runtime_tab_id,
        session_id: "session".into(),
        binding_nonce: "nonce".into(),
        provider: "chat".into(),
        model: None,
        started_at_ms: 1,
    }
}

fn mark_router(tab: &mut Tab, owner: &str, epoch: &ConciergeEpoch) {
    tab.audience = TabAudience::TranscriptRouter;
    tab.spawned_by_tab_id = Some(owner.into());
    tab.hidden = true;
    tab.show_in_tabbar = false;
    tab.agent_type = Some(epoch.provider.clone());
    tab.agent_session_id = Some(epoch.session_id.clone());
    tab.session_binding_nonce = Some(epoch.binding_nonce.clone());
    tab.reported_session_binding_nonce = Some(epoch.binding_nonce.clone());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: epoch.provider.clone(),
        session_id: epoch.session_id.clone(),
    };
}

#[test]
fn superseded_and_foreign_bindings_cannot_read_current_epoch() {
    let root = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(root.path(), "owner", DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let epoch = epoch("runtime".into());
    store.stage_epoch(0, epoch.clone()).unwrap();
    let binding = store
        .set_binding(
            0,
            BindingTarget::Attached {
                epoch_id: epoch.epoch_id.clone(),
            },
            CursorStart::Beginning,
        )
        .unwrap();
    assert_eq!(active_epoch(&store, "owner", &binding).unwrap(), epoch);
    assert!(active_epoch(&store, "other", &binding).is_err());

    store
        .set_binding(
            binding.revision,
            BindingTarget::External {
                tab_id: "override".into(),
            },
            CursorStart::Beginning,
        )
        .unwrap();
    assert!(active_epoch(&store, "owner", &binding).is_err());
    assert!(active_epoch(&store, "owner", &store.binding()).is_err());
}

#[test]
fn replacement_nonce_cannot_read_the_old_runtime_as_current() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let epoch = epoch(tab_id.clone());
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    mark_router(tab, "owner", &epoch);
    assert!(is_current_epoch(tab, &epoch));

    tab.session_binding_nonce = Some("fresh".into());
    tab.reported_session_binding_nonce = Some("fresh".into());
    assert!(!is_current_epoch(tab, &epoch));
}

fn attach(store: &TranscriptStore, epoch: &ConciergeEpoch, cwd: &str) -> TranscriptBinding {
    let revision = store.binding().revision;
    store
        .stage_attached_epoch(
            revision,
            epoch.clone(),
            ContextEpochArchive {
                provider: epoch.provider.clone(),
                session_id: epoch.session_id.clone(),
                cwd: cwd.into(),
                transport: ContextEpochTransport::ChatBackend,
                boundary: ContextEpochBoundary::ContextReset,
            },
        )
        .unwrap();
    store
        .set_binding(
            revision,
            BindingTarget::Attached {
                epoch_id: epoch.epoch_id.clone(),
            },
            CursorStart::Beginning,
        )
        .unwrap()
}

#[test]
fn dismount_preserves_the_fence_and_logical_cursor_until_archive() {
    use crate::service::live_transcript::TranscriptInput;
    let root = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(root.path(), "owner", DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let old = epoch("runtime".into());
    let binding = attach(&store, &old, "work");
    for index in 1..=5 {
        store
            .append(
                "owner",
                TranscriptInput {
                    at_ms: index,
                    content: TranscriptContent::Speech {
                        text: format!("row-{index}"),
                    },
                },
            )
            .unwrap();
    }
    store.ack(&binding, 4).unwrap();
    let suspended = store.suspend_attached_epoch(&binding).unwrap();
    assert!(store.ack(&binding, 5).is_err());
    drop(store);

    let store = TranscriptStore::open(root.path(), "owner", DEFAULT_RAW_BUDGET_BYTES).unwrap();
    assert_eq!(store.binding(), suspended);
    assert_eq!(store.suspended_epoch().unwrap().unwrap().epoch, old);
    let bootstrap = router_bootstrap(&store, "owner", "Main").unwrap();
    let context = bootstrap
        .split("[CONTEXT]\n")
        .nth(1)
        .unwrap()
        .split("[ACK]")
        .next()
        .unwrap();
    let rows: Vec<crate::service::live_transcript::TranscriptRow> = context
        .lines()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    assert_eq!(
        rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert!(bootstrap.ends_with("[ACK]\nconcierge:owner 4\n"));

    let mut fresh = epoch("fresh-tab".into());
    fresh.epoch_id = "fresh".into();
    fresh.session_id = "fresh-session".into();
    fresh.binding_nonce = "fresh-nonce".into();
    let target = BindingTarget::Attached {
        epoch_id: fresh.epoch_id.clone(),
    };
    store
        .stage_epoch(suspended.revision, fresh.clone())
        .unwrap();
    assert!(store
        .set_binding(suspended.revision, target.clone(), CursorStart::Latest)
        .is_err());
    let source = store.epoch_archive_source(&old).unwrap();
    store
        .archive_epoch(
            &old,
            EpochArchive {
                history_ref: serde_json::to_string(&source).unwrap(),
                ended_at_ms: 2,
            },
        )
        .unwrap();
    let binding = store
        .set_binding(suspended.revision, target, CursorStart::Latest)
        .unwrap();
    assert_eq!(active_epoch(&store, "owner", &binding).unwrap(), fresh);
    assert_eq!(store.cursor(&binding).unwrap().acked_seq, 4);
    assert!(store.suspended_epoch().unwrap().is_none());
    let mut replayed = old;
    replayed.epoch_id = "different-epoch".into();
    assert!(store
        .stage_attached_epoch(binding.revision, replayed, source)
        .is_err());
}

#[test]
fn unresolved_archive_refuses_a_replacement_binding() {
    let (mux, owner) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let root = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(root.path(), &owner, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let old = epoch("departed".into());
    let binding = attach(&store, &old, "unavailable");
    store.mark_epoch_admitted(&binding, &old).unwrap();
    store.suspend_attached_epoch(&binding).unwrap();
    let pending = store.suspended_epoch().unwrap().unwrap();
    assert!(fixture
        .service
        .ensure_transcript_epoch_archive(&store, &owner, &pending)
        .is_err());
    assert!(store.suspended_epoch().unwrap().unwrap().archive.is_none());
    assert!(store.ack(&binding, 0).is_err());
}

#[test]
fn closed_history_lineage_is_exact_after_a_new_live_id() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(root.path(), &owner, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let initial = attach(&store, &epoch("runtime".into()), "work");
    mux.close_tab(&owner).unwrap();
    let closed = mux.committed_transcript_owner(&owner).unwrap().unwrap();
    store.close_owner(&owner, closed.clone()).unwrap();
    let (workspace, reopened) = if mux.get_workspace(&workspace).is_some() {
        let result = mux
            .reopen_closed_tab_with_lineage(&workspace, None)
            .unwrap();
        (workspace, result)
    } else {
        mux.reopen_orphaned_tab_with_lineage(&owner).unwrap()
    };
    assert_ne!(owner, reopened.live_tab_id);
    assert_eq!(reopened.closed, closed);
    assert_eq!(
        mux.reopened_transcript_owner(&reopened.live_tab_id)
            .unwrap(),
        Some(closed.clone())
    );
    let mut foreign = closed.clone();
    foreign.group_id = "other".into();
    assert!(store.reopen_owner(&foreign, &reopened.live_tab_id).is_err());
    store.reopen_owner(&closed, &reopened.live_tab_id).unwrap();
    assert!(store.ack(&initial, 0).is_err());
    assert_eq!(
        store.attached_cursor().unwrap().consumer_key,
        format!("concierge:{owner}")
    );
    assert!(mux.get_workspace(&workspace).is_some());
}

#[test]
fn workspace_selection_and_close_fallback_skip_the_attached_router() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let router = mux.create_tab(&workspace).unwrap();
    let other = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&router).unwrap();
    tab.audience = TabAudience::TranscriptRouter;
    tab.hidden = true;
    tab.show_in_tabbar = false;
    mux.set_focused_tab(&owner).unwrap();
    mux.set_active_tab(&workspace, &owner).unwrap();
    assert!(mux.set_focused_tab(&router).is_err());
    assert!(mux.set_active_tab(&workspace, &router).is_err());
    assert!(!mux.set_tab_focus_requested(&router, true));
    mux.close_tab(&owner).unwrap();
    assert_eq!(
        mux.get_workspace(&workspace)
            .unwrap()
            .active_tab_id
            .as_deref(),
        Some(other.as_str())
    );
    assert_eq!(mux.get_focused_tab_id(), Some(other.as_str()));
}

#[test]
fn router_requires_a_single_use_send_id_and_exact_runtime_epoch() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let router = mux.create_tab(&workspace).unwrap();
    let epoch = epoch(router.clone());
    let tab = mux.get_tab_mut(&router).unwrap();
    mark_router(tab, &owner, &epoch);
    let fixture = ServiceFixture::new(mux);
    let root = tempfile::tempdir().unwrap();
    let store = TranscriptStore::open(root.path(), &owner, DEFAULT_RAW_BUDGET_BYTES).unwrap();
    let binding = attach(&store, &epoch, "work");
    assert!(fixture
        .service
        .require_public_transcript_tab(&router)
        .is_err());
    assert!(fixture
        .service
        .require_public_transcript_session(&epoch.provider, &epoch.session_id)
        .is_err());
    assert!(fixture
        .service
        .require_transcript_handoff(&router, "foreign")
        .is_err());
    let admission = Arc::new(TranscriptHandoff {
        owner,
        store,
        binding,
        epoch,
        state: AtomicU8::new(0),
    });
    fixture
        .service
        .ext_or_init(TranscriptHandoffs::default)
        .0
        .lock()
        .insert("outbox-send".into(), Arc::downgrade(&admission));
    let witness = fixture
        .service
        .require_transcript_handoff(&router, "outbox-send")
        .unwrap()
        .unwrap();
    assert!(witness.claim(&fixture.service).is_ok());
    assert!(witness.claim(&fixture.service).is_err());
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&router)
        .unwrap()
        .session_binding_nonce = Some("replacement".into());
    assert!(witness.claim(&fixture.service).is_err());
}

#[tokio::test]
async fn cap_counts_distinct_settled_turns_on_only_the_ready_epoch() {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let router = mux.create_tab(&workspace).unwrap();
    let epoch = epoch(router.clone());
    let tab = mux.get_tab_mut(&router).unwrap();
    mark_router(tab, &owner, &epoch);
    let tab = tab.clone();
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new(&epoch.provider),
    ));
    assert!(capture_epoch(&owner, &tab).is_err());
    let adapter = fixture.service.agent_registry.provider_arc(&epoch.provider);
    let _runtime = engine::test_support::loaded_runtime(&tab, adapter);
    let session = engine::for_tab(&router).unwrap();
    assert!(capture_epoch(&owner, &tab).is_ok());
    restart::arm_attached_transcript_turns(&fixture.service, &epoch).unwrap();
    let store = fixture.service.open_live_transcript(&owner).unwrap();
    let binding = attach(&store, &epoch, "work");
    let cap = DEFAULT_TRANSCRIPT_TURN_LIMIT.get();
    for index in 0..cap - 1 {
        engine::record_completed_turn(&session, &epoch.session_id, Some(&index.to_string()));
    }
    engine::record_completed_turn(&session, &epoch.session_id, Some("0"));
    engine::record_completed_turn(&session, "prior-session", Some("last"));
    assert_eq!(
        restart::attached_transcript_settled_turns(&fixture.service, &epoch).unwrap(),
        cap - 1
    );
    engine::record_completed_turn(&session, &epoch.session_id, Some("last"));
    assert_eq!(
        restart::attached_transcript_settled_turns(&fixture.service, &epoch).unwrap(),
        cap
    );
    session.context.lock().active_turn_id = Some("busy".into());
    assert!(restart::attached_transcript_settled_turns(&fixture.service, &epoch).is_err());
    session.context.lock().active_turn_id = None;
    store.mark_epoch_admitted(&binding, &epoch).unwrap();
    let service = fixture.service.clone();
    let result = tokio::task::spawn_blocking(move || {
        service.admit_attached_transcript_turn(&owner, NonZeroUsize::new(cap - 1).unwrap())
    })
    .await
    .unwrap();
    assert!(result.is_err());
    assert_eq!(store.suspended_epoch().unwrap().unwrap().epoch, epoch);
    assert!(store.ack(&binding, 0).is_err());
}

#[test]
fn history_pages_expose_only_thinking_and_tool_rows() {
    let rows = ["user", "thinking", "assistant", "tool_call", "tool_result"].map(|kind| ChatMsg {
        kind: kind.into(),
        ..Default::default()
    });
    let (first, next) = thinking_and_tools_page(rows.to_vec(), 0, 2);
    assert_eq!(
        first
            .iter()
            .map(|row| row.kind.as_str())
            .collect::<Vec<_>>(),
        ["thinking", "tool_call"]
    );
    let (last, end) = thinking_and_tools_page(rows.to_vec(), next.unwrap(), 2);
    assert_eq!(last[0].kind, "tool_result");
    assert_eq!(end, None);
}
