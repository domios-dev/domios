use std::fs;

use tempfile::TempDir;

use super::*;
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
        .append(
            key.clone(),
            LifecycleTransition::EpochOpened,
        )
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
fn committed_leaf(tab_id: &str, epoch: u64, operation: &str, session_id: &str) -> SplitNodeSnapshot {
    let mut node = leaf(tab_id);
    if let SplitNodeSnapshot::Leaf {
        agent_type,
        agent_type_locked,
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
        *agent_type = Some("claude".into());
        *agent_type_locked = true;
        *claude_session_id = Some(session_id.into());
        *agent_identity = Some(identity(Some(epoch), Some(operation)));
        *role = Some("Manager".into());
        *role_profile = Some("orchestrator".into());
        *team_id = Some("team-1".into());
        *can_spawn = true;
        *depth = 1;
    }
    node
}

/// The verified db79d2da residue: role authority and a launch nonce, with no
/// provider, session, Team, or boot stage ever committed.
fn partial_launch_leaf(tab_id: &str) -> SplitNodeSnapshot {
    let mut node = leaf(tab_id);
    if let SplitNodeSnapshot::Leaf {
        agent_identity,
        role,
        can_spawn,
        depth,
        ..
    } = &mut node
    {
        *agent_identity = Some(AgentIdentitySnapshot {
            reported_session_binding_nonce: None,
            ..identity(None, None)
        });
        *role = Some("Manager".into());
        *can_spawn = true;
        *depth = 1;
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

fn authority_of(snapshot: &SessionSnapshot, tab_id: &str) -> SplitNodeSnapshot {
    snapshot.workspaces[0]
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)
        .map(|tab| tab.split_tree.clone())
        .expect("tab in snapshot")
}

fn verdict_for(outcomes: &[TabRecoveryOutcome], tab_id: &str) -> RecoveryVerdict {
    outcomes
        .iter()
        .find(|outcome| outcome.tab_id == tab_id)
        .map(|outcome| outcome.verdict.clone())
        .expect("verdict for tab")
}

#[test]
fn partial_launch_residue_fails_closed() {
    let (journal, _dir) = new_journal();
    let key = key("db79d2da", 1, "op-partial");
    journal
        .append(
            key.clone(),
            LifecycleTransition::EpochOpened,
        )
        .unwrap();

    let mut snapshot = snapshot(vec![partial_launch_leaf("db79d2da")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());

    assert_eq!(
        verdict_for(&outcomes, "db79d2da"),
        RecoveryVerdict::Stripped(RecoveryRefusal::UnbackedRoleAuthority)
    );
    let SplitNodeSnapshot::Leaf {
        role,
        can_spawn,
        depth,
        agent_type,
        claude_session_id,
        team_id,
        ..
    } = authority_of(&snapshot, "db79d2da")
    else {
        panic!("expected leaf");
    };
    assert_eq!(role, None, "a role-bearing plain shell must not come back");
    assert!(!can_spawn);
    assert_eq!(depth, 0);
    assert_eq!(agent_type, None);
    assert_eq!(claude_session_id, None);
    assert_eq!(team_id, None);
}

#[test]
fn a_full_claim_with_no_checkpoint_is_refused() {
    let (journal, _dir) = new_journal();
    let mut snapshot = snapshot(vec![committed_leaf("tab-a", 1, "op-a", "session-a")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::NoCommittedCheckpoint)
    );
}

#[test]
fn atomic_commit_restores_every_authority_field() {
    let (journal, _dir) = new_journal();
    let key = key("tab-a", 1, "op-a");
    commit_full_launch(&journal, &key, "session-a");

    let mut snapshot = snapshot(vec![committed_leaf("tab-a", 1, "op-a", "session-a")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());

    let RecoveryVerdict::Restored(authority) = verdict_for(&outcomes, "tab-a") else {
        panic!("exact checkpoint must restore");
    };
    assert_eq!(authority.provider, "claude");
    assert_eq!(authority.session_id, "session-a");
    assert_eq!(authority.role.as_deref(), Some("Manager"));
    assert_eq!(authority.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(authority.team_id.as_deref(), Some("team-1"));
    assert!(authority.can_spawn);
    assert_eq!(authority.depth, 1);
    let SplitNodeSnapshot::Leaf { agent_type, .. } = authority_of(&snapshot, "tab-a") else {
        panic!("expected leaf");
    };
    assert_eq!(agent_type.as_deref(), Some("claude"));
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
    let mut snapshot = snapshot(vec![committed_leaf("tab-a", 1, "op-a", "session-a")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::NoCommittedCheckpoint)
    );
}

#[test]
fn restart_restores_only_the_snapshot_of_its_own_epoch() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    journal
        .append(
            key("tab-a", 1, "op-a"),
            LifecycleTransition::RestartRequested,
        )
        .unwrap();
    commit_full_launch(&journal, &key("tab-a", 2, "op-b"), "session-b");

    let load = journal.load();
    let mut current = snapshot(vec![committed_leaf("tab-a", 2, "op-b", "session-b")]);
    assert!(matches!(
        verdict_for(&gate_session_snapshot(&mut current, &load), "tab-a"),
        RecoveryVerdict::Restored(_)
    ));

    let mut stale = snapshot(vec![committed_leaf("tab-a", 1, "op-a", "session-a")]);
    assert_eq!(
        verdict_for(&gate_session_snapshot(&mut stale, &load), "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::EpochMismatch {
            snapshot: LifecycleEpoch(1),
            checkpoint: LifecycleEpoch(2),
        })
    );
}

#[test]
fn same_epoch_different_operation_is_refused() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");

    let mut snapshot = snapshot(vec![committed_leaf("tab-a", 1, "op-foreign", "session-a")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::OperationMismatch {
            snapshot: "op-foreign".into(),
            checkpoint: "op-a".into(),
        })
    );
}

#[test]
fn snapshot_claiming_a_different_session_is_refused() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");

    let mut snapshot = snapshot(vec![committed_leaf("tab-a", 1, "op-a", "session-other")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::FieldMismatch {
            field: "agent_session_id"
        })
    );
}

#[test]
fn a_newer_tabs_checkpoint_never_authorizes_another_tab() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-other", 1, "op-other"), "session-other");

    let mut snapshot = snapshot(vec![committed_leaf("db79d2da", 1, "op-other", "session-other")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "db79d2da"),
        RecoveryVerdict::Stripped(RecoveryRefusal::NoCommittedCheckpoint)
    );
}

#[test]
fn legacy_snapshot_without_lineage_is_unproven() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");

    let mut node = committed_leaf("tab-a", 1, "op-a", "session-a");
    if let SplitNodeSnapshot::Leaf { agent_identity, .. } = &mut node {
        *agent_identity = Some(identity(None, None));
    }
    let mut snapshot = snapshot(vec![node]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::UnprovenLineage)
    );
}

#[test]
fn truncated_tail_refuses_every_claim() {
    let (journal, dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let path = dir.path().join(JOURNAL_FILE_NAME);
    let mut body = fs::read_to_string(&path).unwrap();
    body.push_str("{\"record\":\"transition\",\"entry\":{\"seq\":99,\"key\"");
    fs::write(&path, body).unwrap();

    let load = journal.load();
    assert!(!load.tail.is_intact());
    let mut snapshot = snapshot(vec![committed_leaf("tab-a", 1, "op-a", "session-a")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &load);
    assert!(matches!(
        verdict_for(&outcomes, "tab-a"),
        RecoveryVerdict::Stripped(RecoveryRefusal::JournalTailRefused { .. })
    ));
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
        .append(key("tab-b", 1, "op-b"), LifecycleTransition::Closed)
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
fn compaction_is_equivalent_for_recovery() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    commit_full_launch(&journal, &key("tab-a", 2, "op-b"), "session-b");
    journal
        .append(
            key("tab-a", 3, "op-c"),
            LifecycleTransition::EpochOpened,
        )
        .unwrap();

    let mut before = snapshot(vec![committed_leaf("tab-a", 2, "op-b", "session-b")]);
    let before_verdict = verdict_for(&gate_session_snapshot(&mut before, &journal.load()), "tab-a");
    journal.compact().unwrap();
    let mut after = snapshot(vec![committed_leaf("tab-a", 2, "op-b", "session-b")]);
    let after_verdict = verdict_for(&gate_session_snapshot(&mut after, &journal.load()), "tab-a");

    assert!(matches!(before_verdict, RecoveryVerdict::Restored(_)));
    assert_eq!(before_verdict, after_verdict);
}

#[test]
fn appends_continue_the_sequence_after_compaction() {
    let (journal, _dir) = new_journal();
    commit_full_launch(&journal, &key("tab-a", 1, "op-a"), "session-a");
    let highest = journal.load().last_seq();
    journal.compact().unwrap();

    let seq = journal
        .append(
            key("tab-a", 2, "op-b"),
            LifecycleTransition::EpochOpened,
        )
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
fn a_tab_claiming_nothing_needs_no_proof() {
    let (journal, _dir) = new_journal();
    let mut snapshot = snapshot(vec![leaf("plain-shell")]);
    let outcomes = gate_session_snapshot(&mut snapshot, &journal.load());
    assert_eq!(
        verdict_for(&outcomes, "plain-shell"),
        RecoveryVerdict::NoAuthorityClaimed
    );
}
