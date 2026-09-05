use std::fs;

use tempfile::TempDir;

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab, TabCloseReason};
use crate::session::store::{
    AgentIdentitySnapshot, SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
};

fn new_journal() -> (TabLifecycleJournal, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let journal = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    (journal, dir)
}

fn key(tab_id: &str, epoch: u64, operation: &str) -> LifecycleKey {
    LifecycleKey::new(tab_id, LifecycleEpoch(epoch), operation)
}

/// A launch that committed everything and sealed.
fn commit_full_launch(journal: &TabLifecycleJournal, key: &LifecycleKey, session_id: &str) {
    journal
        .append(key.clone(), LifecycleTransition::EpochOpened)
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::ProviderIdentityCommitted {
                provider: "claude".into(),
            },
        )
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::SessionBound {
                session_id: session_id.into(),
                binding_nonce: "nonce-1".into(),
            },
        )
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::RoleBinding {
                role: Some("Manager".into()),
                role_profile: Some("orchestrator".into()),
                can_spawn: true,
                depth: 1,
            },
        )
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::TeamBinding {
                team_id: Some("team-1".into()),
                parent_tab_id: None,
            },
        )
        .unwrap();
    journal
        .append(key.clone(), LifecycleTransition::LaunchReady)
        .unwrap();
}

fn identity(epoch: Option<u64>, operation: Option<&str>) -> AgentIdentitySnapshot {
    AgentIdentitySnapshot {
        provisioned_cwd: None,
        session_binding_nonce: Some("nonce-1".into()),
        reported_session_binding_nonce: Some("nonce-1".into()),
        agent_state: None,
        boot_stage: None,
        reasoning_effort: Default::default(),
        context_epochs: vec![],
        task_binding: None,
        roleless_tracked: false,
        lifecycle_epoch: epoch,
        lifecycle_operation_id: operation.map(str::to_string),
    }
}

fn leaf(tab_id: &str) -> SplitNodeSnapshot {
    SplitNodeSnapshot::Leaf {
        tab_id: tab_id.into(),
        cwd: Some("D:/Developer/codeterm".into()),
        title: Some("claude — orchestrator".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: None,
        agent_type_locked: false,
        claude_session_id: None,
        agent_identity: None,
        role: None,
        role_profile: None,
        team_id: None,
        parent_tab_id: None,
        spawned_by_tab_id: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: false,
        depth: 0,
        chat_view: None,
        auto_approve: false,
        audience: crate::multiplexer::types::TabAudience::Automation,
        agent_session_model: None,
        agent_planned_model: None,
        agent_requested_model: None,
        agent_provider_resolved_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
        watcher: None,
        pending_delivery: None,
    }
}

/// A tab that reached the end of a launch with every field committed.
fn committed_leaf(
    tab_id: &str,
    epoch: u64,
    operation: &str,
    session_id: &str,
) -> SplitNodeSnapshot {
    let mut node = leaf(tab_id);
    if let SplitNodeSnapshot::Leaf {
        agent_type,
        agent_type_locked,
        claude_session_id,
        agent_identity,
        role,
        role_profile,
        team_id,
        parent_tab_id,
        can_spawn,
        depth,
        ..
    } = &mut node
    {
        *agent_type = Some("claude".into());
        *agent_type_locked = true;
        *claude_session_id = Some(session_id.into());
        *agent_identity = Some(identity(Some(epoch), Some(operation)));
        *role = Some("Manager".into());
        *role_profile = Some("orchestrator".into());
        *team_id = Some("team-1".into());
        *parent_tab_id = Some("tab-parent".into());
        *can_spawn = true;
        *depth = 1;
    }
    node
}

/// Residue of a launch that never completed: role authority and a launch
/// nonce, with no provider or session ever committed.
fn partial_launch_leaf(tab_id: &str) -> SplitNodeSnapshot {
    let mut node = leaf(tab_id);
    if let SplitNodeSnapshot::Leaf {
        agent_identity,
        role,
        team_id,
        can_spawn,
        depth,
        ..
    } = &mut node
    {
        *agent_identity = Some(AgentIdentitySnapshot {
            reported_session_binding_nonce: None,
            ..identity(None, None)
        });
        *role = Some("Worker".into());
        *team_id = Some("team-1".into());
        *can_spawn = true;
        *depth = 1;
    }
    node
}

/// A chatBackend-backed tab. Its transport commits no lifecycle transitions,
/// so any lineage it carries is journal-alien.
fn chat_backend_leaf(tab_id: &str, lineage: Option<(u64, &str)>) -> SplitNodeSnapshot {
    let mut node = leaf(tab_id);
    if let SplitNodeSnapshot::Leaf {
        chatbackend_plugin_id,
        chatbackend_session_id,
        agent_type,
        claude_session_id,
        agent_identity,
        role,
        role_profile,
        team_id,
        can_spawn,
        depth,
        ..
    } = &mut node
    {
        *chatbackend_plugin_id = Some("lmstudio".into());
        *chatbackend_session_id = Some("plugin-session".into());
        *agent_type = Some("lmstudio".into());
        *claude_session_id = Some("plugin-session".into());
        *agent_identity = Some(identity(
            lineage.map(|(epoch, _)| epoch),
            lineage.map(|(_, operation)| operation),
        ));
        *role = Some("Worker".into());
        *role_profile = Some("executor".into());
        *team_id = Some("team-1".into());
        *can_spawn = false;
        *depth = 2;
    }
    node
}

fn snapshot(leaves: Vec<SplitNodeSnapshot>) -> SessionSnapshot {
    SessionSnapshot {
        version: "test".into(),
        timestamp: 0,
        active_workspace_id: Some("ws".into()),
        teams: vec![],
        workspaces: vec![WorkspaceSnapshot {
            id: "ws".into(),
            name: "Main".into(),
            active_tab_id: None,
            cwd: None,
            protected: false,
            sort_order: 0,
            tabs: leaves
                .into_iter()
                .enumerate()
                .map(|(index, split_tree)| TabSnapshot {
                    id: split_tree_tab_id(&split_tree),
                    title: "tab".into(),
                    description: None,
                    split_tree,
                    sort_order: index as i32,
                })
                .collect(),
        }],
    }
}

fn split_tree_tab_id(node: &SplitNodeSnapshot) -> String {
    match node {
        SplitNodeSnapshot::Leaf { tab_id, .. } => tab_id.clone(),
        SplitNodeSnapshot::Split { .. } => "split".into(),
    }
}

/// The real restore path. It takes no journal: restore is unconditional.
fn restored_tab(leaves: Vec<SplitNodeSnapshot>, tab_id: &str) -> Tab {
    MultiplexerState::from_session_snapshot(&snapshot(leaves))
        .get_tab(tab_id)
        .cloned()
        .expect("tab survives restore")
}

fn assert_full_authority(tab: &Tab, session_id: &str) {
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.agent_session_id.as_deref(), Some(session_id));
    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(tab.team_id.as_deref(), Some("team-1"));
    assert_eq!(tab.parent_tab_id.as_deref(), Some("tab-parent"));
    assert!(tab.can_spawn);
    assert_eq!(tab.depth, 1);
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some("nonce-1")
    );
}

// ---------------------------------------------------------------------------
// Unconditional restore: snapshot authority survives verbatim regardless of
// journal state. The journal is telemetry; restore never consults it.
// ---------------------------------------------------------------------------

#[test]
fn restore_keeps_every_authority_field_with_no_journal_at_all() {
    let (journal, _dir) = new_journal();
    assert!(checkpoints(&journal.load()).is_empty());

    let tab = restored_tab(
        vec![committed_leaf("tab-a", 1, "op-a", "session-a")],
        "tab-a",
    );

    assert_full_authority(&tab, "session-a");
    assert_eq!(tab.lifecycle_epoch, Some(1));
    assert_eq!(tab.lifecycle_operation_id.as_deref(), Some("op-a"));
}

#[test]
fn restore_keeps_authority_when_the_journal_was_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let first = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    commit_full_launch(&first, &key("tab-a", 1, "op-a"), "session-a");
    fs::remove_file(dir.path().join(JOURNAL_FILE_NAME)).unwrap();
    let rebuilt = TabLifecycleJournal::for_dir(dir.path().to_path_buf());
    assert!(checkpoints(&rebuilt.load()).is_empty());

    let tab = restored_tab(
        vec![committed_leaf("tab-a", 1, "op-a", "session-a")],
        "tab-a",
    );

    assert_full_authority(&tab, "session-a");
}

#[test]
fn restore_keeps_authority_across_epoch_and_operation_mismatch() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 2, "op-b"), "session-b");

    let tab = restored_tab(
        vec![committed_leaf("tab-a", 1, "op-a", "session-a")],
        "tab-a",
    );

    assert_full_authority(&tab, "session-a");
    assert_eq!(tab.lifecycle_epoch, Some(1));
    assert_eq!(tab.lifecycle_operation_id.as_deref(), Some("op-a"));
}

#[test]
fn restore_keeps_authority_with_a_refused_journal_tail() {
    let (journal, dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let mut body = fs::read_to_string(&path).unwrap();
    body.push_str("{\"record\":\"transition\",\"entry\":{\"seq\":99,\"key\"");
    fs::write(&path, body).unwrap();
    assert!(!journal.load().tail.is_intact());

    let tab = restored_tab(
        vec![committed_leaf("tab-a", 1, "op-a", "session-a")],
        "tab-a",
    );

    assert_full_authority(&tab, "session-a");
}

#[test]
fn restore_keeps_pre_journal_authority_without_stripping() {
    let mut node = committed_leaf("tab-legacy", 1, "op-legacy", "session-legacy");
    if let SplitNodeSnapshot::Leaf { agent_identity, .. } = &mut node {
        *agent_identity = Some(identity(None, None));
    }

    let tab = restored_tab(vec![node], "tab-legacy");

    assert_full_authority(&tab, "session-legacy");
    assert_eq!(tab.lifecycle_epoch, None);
    assert_eq!(tab.lifecycle_operation_id, None);
}

#[test]
fn restore_keeps_authority_with_half_a_lineage() {
    let mut node = committed_leaf("tab-a", 1, "op-a", "session-a");
    if let SplitNodeSnapshot::Leaf { agent_identity, .. } = &mut node {
        *agent_identity = Some(identity(Some(1), None));
    }

    let tab = restored_tab(vec![node], "tab-a");

    assert_full_authority(&tab, "session-a");
    assert_eq!(tab.lifecycle_epoch, Some(1));
    assert_eq!(tab.lifecycle_operation_id, None);
}

#[test]
fn restore_keeps_a_rolled_back_operations_snapshot_fields() {
    let (journal, _dir) = new_journal();
    let key = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &key, "session-a");
    journal
        .append(
            key.clone(),
            LifecycleTransition::RolledBack {
                reason: "agent exited".into(),
            },
        )
        .unwrap();
    assert!(checkpoints(&journal.load()).is_empty());

    let tab = restored_tab(
        vec![committed_leaf("tab-a", 1, "op-a", "session-a")],
        "tab-a",
    );

    assert_full_authority(&tab, "session-a");
}

#[test]
fn restore_keeps_a_role_claim_without_provider_identity() {
    let (journal, _dir) = new_journal();
    journal
        .append(
            key("tab-partial", 1, "op-partial"),
            LifecycleTransition::EpochOpened,
        )
        .unwrap();

    let tab = restored_tab(vec![partial_launch_leaf("tab-partial")], "tab-partial");

    assert_eq!(tab.role, Some(AgentRole::Worker));
    assert_eq!(tab.team_id.as_deref(), Some("team-1"));
    assert!(tab.can_spawn);
    assert_eq!(tab.depth, 1);
    assert_eq!(tab.agent_type, None, "no provider is ever invented");
}

#[test]
fn restore_keeps_a_chat_backend_tab_with_lineage() {
    let tab = restored_tab(
        vec![chat_backend_leaf("tab-chat", Some((1, "op-chat")))],
        "tab-chat",
    );

    assert!(tab.is_chat_backend());
    assert_eq!(tab.agent_type.as_deref(), Some("lmstudio"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("plugin-session"));
    assert_eq!(tab.role, Some(AgentRole::Worker));
    assert_eq!(tab.role_profile.as_deref(), Some("executor"));
    assert_eq!(tab.team_id.as_deref(), Some("team-1"));
    assert_eq!(tab.depth, 2);
}

#[test]
fn a_plain_shell_restores_as_a_plain_shell() {
    let tab = restored_tab(vec![leaf("plain-shell")], "plain-shell");

    assert_eq!(tab.agent_type, None);
    assert_eq!(tab.agent_session_id, None);
    assert_eq!(tab.role, None);
    assert_eq!(tab.team_id, None);
    assert!(!tab.can_spawn);
    assert_eq!(tab.depth, 0);
}

// ---------------------------------------------------------------------------
// Journal mechanics: the telemetry record itself stays trustworthy.
// ---------------------------------------------------------------------------

#[test]
fn agent_type_write_provenance_does_not_change_committed_authority() {
    let (journal, _dir) = new_journal();
    let key = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &key, "session-a");
    journal
        .append(
            key,
            LifecycleTransition::AgentTypeWrite {
                origin: AgentTypeWriteOrigin::Parser,
                previous_provider: Some("claude".into()),
                new_provider: Some("claude".into()),
                attempted_provider: Some("codex".into()),
                caller_tab_id: Some("tab-caller".into()),
                session_id: Some("session-a".into()),
                nonce_verified: true,
                applied: false,
            },
        )
        .unwrap();

    let load = journal.load();
    let write = load.records.iter().find_map(|record| match record {
        JournalRecord::Transition { entry }
            if matches!(
                &entry.transition,
                LifecycleTransition::AgentTypeWrite { .. }
            ) =>
        {
            Some(&entry.transition)
        }
        _ => None,
    });
    assert!(matches!(
        write,
        Some(LifecycleTransition::AgentTypeWrite {
            origin: AgentTypeWriteOrigin::Parser,
            previous_provider: Some(previous),
            new_provider: Some(new),
            attempted_provider: Some(attempted),
            caller_tab_id: Some(caller),
            session_id: Some(session),
            nonce_verified: true,
            applied: false,
        }) if previous == "claude"
            && new == "claude"
            && attempted == "codex"
            && caller == "tab-caller"
            && session == "session-a"
    ));
    assert_eq!(checkpoints(&load)[0].authority.provider, "claude");
    assert!(checkpoints(&load)[0].agent_type_write.is_none());
}

#[test]
fn verified_identity_write_advances_checkpoint_and_survives_compaction() {
    let (journal, _dir) = new_journal();
    let key = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &key, "session-a");
    journal
        .append(
            key.clone(),
            LifecycleTransition::AgentTypeWrite {
                origin: AgentTypeWriteOrigin::Repair,
                previous_provider: Some("claude".into()),
                new_provider: Some("codex".into()),
                attempted_provider: Some("codex".into()),
                caller_tab_id: Some("repair-tab".into()),
                session_id: Some("session-a".into()),
                nonce_verified: true,
                applied: true,
            },
        )
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::ProviderIdentityCommitted {
                provider: "codex".into(),
            },
        )
        .unwrap();
    journal
        .append(
            key,
            LifecycleTransition::SessionBound {
                session_id: "session-a".into(),
                binding_nonce: "nonce-1".into(),
            },
        )
        .unwrap();

    let history = checkpoint_history(&journal.load(), "tab-a");
    assert_eq!(history.first().unwrap().authority.provider, "codex");
    assert_eq!(
        history
            .first()
            .unwrap()
            .agent_type_write
            .as_ref()
            .unwrap()
            .origin,
        AgentTypeWriteOrigin::Repair
    );
    assert_eq!(history.last().unwrap().authority.provider, "claude");

    journal.compact().unwrap();
    let load = journal.load();
    assert_eq!(checkpoints(&load)[0].authority.provider, "codex");
    assert_eq!(
        checkpoints(&load)[0]
            .agent_type_write
            .as_ref()
            .unwrap()
            .origin,
        AgentTypeWriteOrigin::Repair
    );
    let compacted_history = checkpoint_history(&load, "tab-a");
    assert_eq!(
        compacted_history.last().unwrap().authority.provider,
        "claude"
    );
    assert!(compacted_history.len() <= PRIOR_CHECKPOINT_LIMIT + 1);
    assert!(load.records.iter().any(|record| matches!(
        record,
        JournalRecord::Transition { entry }
            if matches!(
                &entry.transition,
                LifecycleTransition::AgentTypeWrite {
                    origin: AgentTypeWriteOrigin::Repair,
                    applied: true,
                    ..
                }
            )
    )));
}

#[test]
fn compaction_retains_the_bounded_recent_checkpoint_lineage() {
    let (journal, _dir) = new_journal();
    for epoch in 1..=20 {
        commit_full_launch(
            &journal,
            &key("tab-a", epoch, &format!("op-{epoch}")),
            &format!("session-{epoch}"),
        );
    }

    let before = checkpoint_history(&journal.load(), "tab-a");
    assert_eq!(before.len(), PRIOR_CHECKPOINT_LIMIT + 1);
    assert_eq!(before.first().unwrap().key.epoch, LifecycleEpoch(20));
    assert_eq!(before.last().unwrap().key.epoch, LifecycleEpoch(4));

    journal.compact().unwrap();
    let after = checkpoint_history(&journal.load(), "tab-a");
    assert_eq!(after, before);
}

#[test]
fn unsealed_launch_never_becomes_a_checkpoint() {
    let (journal, _dir) = new_journal();
    let key = key("tab-a", 1, "op-a");
    journal
        .append(
            key.clone(),
            LifecycleTransition::ProviderIdentityCommitted {
                provider: "claude".into(),
            },
        )
        .unwrap();
    journal
        .append(
            key.clone(),
            LifecycleTransition::SessionBound {
                session_id: "session-a".into(),
                binding_nonce: "nonce-1".into(),
            },
        )
        .unwrap();

    assert!(checkpoints(&journal.load()).is_empty());
}

#[test]
fn rollback_withdraws_the_checkpoint() {
    let (journal, _dir) = new_journal();
    let key = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &key, "session-a");
    assert_eq!(checkpoints(&journal.load()).len(), 1);

    journal
        .append(
            key.clone(),
            LifecycleTransition::RolledBack {
                reason: "agent exited".into(),
            },
        )
        .unwrap();

    assert!(checkpoints(&journal.load()).is_empty());
}

#[test]
fn non_monotonic_sequence_refuses_the_rest_of_the_file() {
    let (journal, dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let first = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_string();
    fs::write(&path, format!("{first}\n{first}\n")).unwrap();

    let load = journal.load();
    assert_eq!(load.records.len(), 1);
    assert!(matches!(load.tail, TailStatus::Refused { line: 2, .. }));
}

#[test]
fn compaction_preserves_checkpoints_and_bounds_the_file() {
    let (journal, dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    commit_full_launch(&journal, &key("tab-b", 1, "op-b"), "session-b");
    journal
        .append(
            key("tab-b", 1, "op-b"),
            LifecycleTransition::Closed {
                reason: "user".to_string(),
            },
        )
        .unwrap();
    let before = checkpoints(&journal.load());
    let raw_records = journal.load().records.len();

    let written = journal.compact().unwrap();
    let after_load = journal.load();

    assert!(after_load.tail.is_intact());
    assert!(written < raw_records, "compaction must shrink the file");
    assert_eq!(checkpoints(&after_load), before);
    assert!(
        after_load
            .records
            .iter()
            .all(|record| record.tab_id() != "tab-b"),
        "a closed tab leaves no residue behind"
    );
    let path = dir.path().join(JOURNAL_FILE_NAME);
    assert!(fs::read_to_string(path).unwrap().ends_with('\n'));
}

#[test]
fn repeated_compaction_replaces_in_place_and_leaves_no_temp() {
    let (journal, dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    journal.compact().unwrap();
    commit_full_launch(&journal, &key("tab-a", 2, "op-b"), "session-b");
    journal.compact().unwrap();

    let residue: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(residue.is_empty(), "temp files left behind: {residue:?}");

    let after_compaction = journal.load();
    assert!(after_compaction.tail.is_intact());
    assert_eq!(checkpoints(&after_compaction).len(), 1);
    let seq = journal
        .append(key("tab-a", 3, "op-c"), LifecycleTransition::EpochOpened)
        .unwrap();
    assert!(seq > after_compaction.last_seq());
    assert!(journal.load().tail.is_intact());
}

#[test]
fn compaction_reaches_the_same_checkpoints_as_the_full_history() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    commit_full_launch(&journal, &key("tab-a", 2, "op-b"), "session-b");
    journal
        .append(key("tab-a", 3, "op-c"), LifecycleTransition::EpochOpened)
        .unwrap();

    let before = checkpoints(&journal.load());
    journal.compact().unwrap();
    let after = checkpoints(&journal.load());

    assert_eq!(before.len(), 1);
    assert_eq!(before, after);
}

#[test]
fn appends_continue_the_sequence_after_compaction() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let highest = journal.load().last_seq();
    journal.compact().unwrap();

    let seq = journal
        .append(key("tab-a", 2, "op-b"), LifecycleTransition::EpochOpened)
        .unwrap();

    assert!(seq > highest);
    assert!(journal.load().tail.is_intact());
}

#[test]
fn next_epoch_follows_the_tabs_own_history() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    commit_full_launch(&journal, &key("tab-b", 7, "op-b"), "session-b");

    let load = journal.load();
    assert_eq!(next_epoch_for(&load, "tab-a"), LifecycleEpoch(2));
    assert_eq!(next_epoch_for(&load, "tab-b"), LifecycleEpoch(8));
    assert_eq!(next_epoch_for(&load, "tab-fresh"), LifecycleEpoch::first());
}

#[test]
fn a_close_reason_survives_the_journal_round_trip() {
    let (journal, _dir) = new_journal();
    let k = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &k, "session-a");
    journal
        .append(
            k.clone(),
            LifecycleTransition::Closed {
                reason: TabCloseReason::Watchdog.label().to_string(),
            },
        )
        .unwrap();

    // Bind the load: `closed` borrows from it, so it must outlive the statement.
    let loaded = journal.load();
    let closed: Vec<&LifecycleTransition> = loaded
        .records
        .iter()
        .filter_map(|record| match record {
            JournalRecord::Transition { entry } => Some(&entry.transition),
            _ => None,
        })
        .filter(|transition| matches!(transition, LifecycleTransition::Closed { .. }))
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(
        closed[0],
        &LifecycleTransition::Closed {
            reason: TabCloseReason::Watchdog.label().to_string(),
        },
        "the journal must name the seam that closed the tab"
    );
}

#[test]
fn a_close_record_written_before_attribution_reads_as_unattributed() {
    let decoded: LifecycleTransition = serde_json::from_str(r#"{"transition":"closed"}"#).unwrap();
    assert_eq!(
        decoded,
        LifecycleTransition::Closed {
            reason: "unattributed".to_string(),
        },
        "a pre-attribution journal line still loads, and says so"
    );
    assert_ne!(
        decoded,
        LifecycleTransition::Closed {
            reason: TabCloseReason::User.label().to_string(),
        },
        "an unattributed close must never be read back as a user close"
    );
}
