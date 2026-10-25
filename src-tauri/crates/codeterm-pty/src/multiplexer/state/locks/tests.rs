use super::*;
use codeterm_entitlements::compiled_free;

fn free() -> LimitGate {
    LimitGate::new(compiled_free(), true)
}

fn three_workspaces() -> (MultiplexerState, String, Vec<String>) {
    let (mut mux, _) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    let ids = ["a", "b", "c"]
        .map(|name| mux.create_workspace(name.into(), None).id)
        .to_vec();
    mux.set_limit_gate(LimitGate::dormant());
    (mux, general, ids)
}

#[test]
fn newest_workspace_locks_and_reordering_does_not_move_the_lock() {
    let (mut mux, general, ids) = three_workspaces();
    assert!(mux.set_limit_gate(free()).locks);
    let before = mux.locks();
    assert_eq!(
        before.workspaces.iter().collect::<Vec<_>>(),
        [ids[2].as_str()]
    );
    mux.move_workspace_to_front(&ids[2]);
    assert_eq!(mux.locks(), before);
    assert!(!mux.locks().is_locked(LimitKey::Workspaces, &general));
}

#[test]
fn payloads_mark_locked_items_and_still_list_every_item() {
    let (mut mux, _, ids) = three_workspaces();
    let listed = mux.list_workspaces().len();
    mux.set_limit_gate(free());
    let infos = mux.list_workspaces();
    assert_eq!(infos.len(), listed);
    assert!(infos.iter().any(|info| info.id == ids[2] && info.locked));
    let snapshot = mux.snapshot();
    assert_eq!(snapshot.workspaces.len(), listed);
    assert!(snapshot
        .workspaces
        .iter()
        .any(|workspace| workspace.id == ids[2] && workspace.locked));
    let wire = serde_json::to_value(&snapshot).unwrap();
    assert!(wire["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|workspace| workspace["locked"] == true));
}

#[test]
fn entitlement_flip_toggles_locks_without_rebuilding_the_mux() {
    let (mut mux, _, ids) = three_workspaces();
    assert_eq!(
        mux.set_limit_gate(LimitGate::dormant()),
        LimitGateChange::default()
    );
    assert!(mux.locks().workspaces.is_empty());
    assert!(mux.set_limit_gate(free()).locks);
    assert!(mux.locks().is_locked(LimitKey::Workspaces, &ids[2]));
    assert!(mux.set_limit_gate(LimitGate::dormant()).locks);
    assert!(mux.locks().workspaces.is_empty());
}

#[test]
fn restore_backfills_legacy_creation_order_once_in_snapshot_order() {
    let (mut mux, _, _) = three_workspaces();
    let mut session = mux.to_session_snapshot();
    for workspace in &mut session.workspaces {
        workspace.created_at_ms = 0;
    }
    let restored = MultiplexerState::from_session_snapshot(&session);
    let stamps: Vec<u64> = restored
        .workspaces
        .iter()
        .map(|workspace| workspace.created_at_ms)
        .collect();
    assert_eq!(stamps, (1..=stamps.len() as u64).collect::<Vec<_>>());
    let resaved = restored.to_session_snapshot();
    assert!(resaved.workspaces.iter().all(|w| w.created_at_ms > 0));
    mux.set_limit_gate(LimitGate::dormant());
}

#[test]
fn writes_into_a_locked_workspace_tab_or_team_are_refused_and_reads_stay() {
    let (mut mux, _, ids) = three_workspaces();
    let tab = mux.create_tab(&ids[2]).unwrap();
    assert!(mux.tab_write_lock(&tab).is_none());
    mux.set_limit_gate(free());
    let refusal = mux.tab_write_lock(&tab).unwrap();
    assert_eq!(refusal.limit, LimitKey::Workspaces);
    assert!(refusal
        .to_string()
        .starts_with(codeterm_entitlements::PLAN_LIMIT_LOCKED));
    assert!(mux
        .create_tab(&ids[2])
        .unwrap_err()
        .starts_with(codeterm_entitlements::PLAN_LIMIT_LOCKED));
    assert!(mux.claim_existing_tab(&ids[2], &tab).is_err());
    assert!(mux.get_tab(&tab).is_some());
    assert!(mux.create_tab(&ids[0]).is_ok());
    mux.set_limit_gate(LimitGate::dormant());
    assert!(mux.tab_write_lock(&tab).is_none());
    assert!(mux.create_tab(&ids[2]).is_ok());
}

#[test]
fn locked_tab_and_workspace_metadata_refuse_and_keep_their_values() {
    let (mut mux, _, ids) = three_workspaces();
    let tab = mux.create_tab(&ids[2]).unwrap();
    mux.rename_tab(&tab, "Before").unwrap();
    mux.set_tab_description(&tab, Some("before".into()))
        .unwrap();
    mux.set_limit_gate(free());
    let locked = |result: Result<(), String>| {
        assert!(result
            .unwrap_err()
            .starts_with(codeterm_entitlements::PLAN_LIMIT_LOCKED));
    };
    locked(mux.rename_tab(&tab, "After"));
    locked(mux.rename_tab_at_level(&tab, "After", crate::multiplexer::types::TitleLevel::Agent));
    locked(mux.set_tab_description(&tab, Some("after".into())));
    locked(mux.rename_workspace(&ids[2], "After"));
    let kept = mux.get_tab(&tab).unwrap();
    assert_eq!(kept.title, "Before");
    assert_eq!(kept.description.as_deref(), Some("before"));
    assert!(mux.rename_workspace(&ids[0], "Unlocked").is_ok());
    mux.set_limit_gate(LimitGate::dormant());
    assert!(mux.rename_tab(&tab, "After").is_ok());
}

#[test]
fn production_mux_constructors_enforce_before_billing_starts() {
    let (mut mux, _) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    let ids = ["a", "b", "c"].map(|name| mux.create_workspace(name.into(), None).id);
    assert!(mux.item_write_lock(LimitKey::Workspaces, &ids[2]).is_some());
    let restored = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    assert_eq!(restored.locks(), mux.locks());
    mux.set_limit_gate(LimitGate::dormant());
    assert!(restored
        .item_write_lock(LimitKey::Workspaces, &ids[2])
        .is_some());
}

#[test]
fn locked_team_rename_and_lifecycle_keep_original_metadata() {
    let mut mux = MultiplexerState::empty();
    let workspace = mux.create_workspace("Project".into(), None).id;
    let mut team = codeterm_types::team::Team::new(&workspace, 1);
    team.name = Some("Original".into());
    mux.upsert_team(team.clone());
    let mut access = compiled_free();
    access.limits.teams.max = Some(0);
    mux.set_limit_gate(LimitGate::new(access, true));
    assert!(mux
        .rename_team(&team.id, "Changed".into())
        .unwrap_err()
        .starts_with("plan_limit_locked: "));
    assert!(mux
        .set_team_lifecycle(&team.id, codeterm_types::team::TeamLifecycle::Active)
        .unwrap_err()
        .starts_with("plan_limit_locked: "));
    assert_eq!(mux.get_team(&team.id).unwrap(), &team);
}
