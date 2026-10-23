use super::*;
use codeterm_entitlements::{compiled_free, Authority, EffectiveAccess, Perks, Scope, ScopedLimit};

fn paid() -> EffectiveAccess {
    let mut access = compiled_free();
    access.authority = Authority::Billing;
    access.perks = Perks::default();
    for limit in [
        &mut access.limits.workspaces,
        &mut access.limits.teams,
        &mut access.limits.agent_tabs,
    ] {
        *limit = ScopedLimit {
            max: None,
            scope: Scope::Installation,
        };
    }
    access
}

fn candidates(prefix: &str, count: u64) -> Vec<LockCandidate> {
    (0..count)
        .map(|n| LockCandidate {
            id: format!("{prefix}-{n:02}"),
            created_at: n,
            exempt: false,
        })
        .collect()
}

#[test]
fn lock_state_follows_the_entitlement_snapshot_without_restart() {
    let items = LockItems {
        workspaces: candidates("ws", 3),
        teams: candidates("team", 7),
        agent_tabs: candidates("tab", 36),
    };
    let free = LimitGate::new(compiled_free(), true);
    let upgraded = LimitGate::new(paid(), true);
    let states: Vec<bool> = [&free, &upgraded, &free]
        .into_iter()
        .map(|gate| LockProjection::project(gate, &items).is_locked(LimitKey::Teams, "team-06"))
        .collect();
    assert_eq!(states, [true, false, true]);
    let projection = LockProjection::project(&free, &items);
    assert!(projection.is_locked(LimitKey::Workspaces, "ws-02"));
    assert!(projection.is_locked(LimitKey::AgentTabs, "tab-35"));
    assert!(!projection.is_locked(LimitKey::AgentTabs, "tab-34"));
    assert_eq!(
        LockProjection::project(&LimitGate::dormant(), &items),
        LockProjection::default()
    );
}

#[test]
fn projection_from_mux_marks_without_hiding_and_exempts_general() {
    let (mut mux, _) = MultiplexerState::new();
    let general = mux.list_workspaces()[0].id.clone();
    mux.set_workspace_protected(&general, true);
    for name in ["a", "b", "c"] {
        mux.create_workspace(name.into(), None);
    }
    let before = mux.list_workspaces().len();
    let items = LockItems::from_mux(&mux);
    let projection = LockProjection::project(&LimitGate::new(compiled_free(), true), &items);
    assert_eq!(projection.workspaces.len(), 1);
    assert!(!projection.is_locked(LimitKey::Workspaces, &general));
    assert_eq!(mux.list_workspaces().len(), before);
    assert_eq!(items.workspaces.len(), before);
}
