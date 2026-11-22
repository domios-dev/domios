use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::live_transcript::{CursorStart, DEFAULT_RAW_BUDGET_BYTES};
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
    assert!(is_current_epoch(tab, &epoch));

    tab.session_binding_nonce = Some("fresh".into());
    tab.reported_session_binding_nonce = Some("fresh".into());
    assert!(!is_current_epoch(tab, &epoch));
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
