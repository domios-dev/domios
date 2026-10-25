use crate::gate::{
    admit, LimitGate, LimitKey, LimitRefusal, LockCandidate, LockedRefusal, FREE_LIMIT_REACHED,
    PLAN_LIMIT_LOCKED,
};
use crate::payload::{Authority, Perks, Scope, ScopedLimit};
use crate::verdict::{compiled_free, EffectiveAccess};

fn unlimited() -> EffectiveAccess {
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

#[test]
fn dormant_gate_admits_past_every_free_limit() {
    let free = compiled_free();
    for limit in [LimitKey::Workspaces, LimitKey::Teams, LimitKey::AgentTabs] {
        assert_eq!(admit(limit, 10_000, &free, false), Ok(()));
    }
}

#[test]
fn enforced_free_gate_admits_below_and_refuses_at_the_limit() {
    let free = compiled_free();
    for (limit, max) in [
        (LimitKey::Workspaces, 2),
        (LimitKey::Teams, 6),
        (LimitKey::AgentTabs, 35),
    ] {
        assert_eq!(admit(limit, max - 1, &free, true), Ok(()), "{limit:?}");
        assert_eq!(
            admit(limit, max, &free, true),
            Err(LimitRefusal {
                limit,
                count: max,
                max
            })
        );
        assert_eq!(
            admit(limit, max + 4, &free, true).unwrap_err().count,
            max + 4
        );
    }
}

#[test]
fn unlimited_access_is_never_refused() {
    let access = unlimited();
    for limit in [LimitKey::Workspaces, LimitKey::Teams, LimitKey::AgentTabs] {
        assert_eq!(admit(limit, u64::MAX, &access, true), Ok(()));
    }
}

#[test]
fn refusal_leads_with_the_stable_code_and_names_count_and_max() {
    let text = LimitRefusal {
        limit: LimitKey::Teams,
        count: 6,
        max: 6,
    }
    .to_string();
    assert!(text.starts_with(&format!("{FREE_LIMIT_REACHED}: ")));
    assert!(text.contains("at most 6 Teams across all workspaces (you have 6)"));
    let tabs = LimitRefusal {
        limit: LimitKey::AgentTabs,
        count: 35,
        max: 35,
    }
    .to_string();
    assert!(tabs.contains("at most 35 "));
    assert!(tabs.contains("(you have 35)"));
    assert!(tabs.ends_with("(limit: agent.tabs)"));
    assert_eq!(LimitKey::AgentTabs.key(), "agent.tabs");
}

#[test]
fn summary_reports_enforcement_and_the_three_maxima() {
    let dormant = LimitGate::dormant().summary();
    assert!(!dormant.enforcing);
    assert_eq!(
        (dormant.workspaces, dormant.teams, dormant.agent_tabs),
        (Some(2), Some(6), Some(35))
    );
    let paid = LimitGate::new(unlimited(), true).summary();
    assert!(paid.enforcing);
    assert_eq!(
        (paid.workspaces, paid.teams, paid.agent_tabs),
        (None, None, None)
    );
    assert_eq!(
        serde_json::to_value(dormant).unwrap(),
        serde_json::json!({"enforcing": false, "mesh.machines": 2, "mem.items": 500, "workspaces": 2, "teams": 6, "agent.tabs": 35})
    );
}

fn candidates(count: u64) -> Vec<LockCandidate> {
    (0..count)
        .map(|n| LockCandidate {
            id: format!("item-{n:03}"),
            created_at: 1_000 + n,
            exempt: false,
        })
        .collect()
}

fn enforced_free() -> LimitGate {
    LimitGate::new(compiled_free(), true)
}

#[test]
fn workspaces_lock_the_newest_beyond_max_and_never_general() {
    let mut items = candidates(3);
    items.insert(
        0,
        LockCandidate {
            id: "general".into(),
            created_at: u64::MAX,
            exempt: true,
        },
    );
    let locked = enforced_free().over_limit(LimitKey::Workspaces, &items);
    assert_eq!(locked.iter().collect::<Vec<_>>(), ["item-002"]);
    assert!(!locked.contains("general"));
    assert_eq!(items.len(), 4);
}

#[test]
fn tabs_and_teams_lock_exactly_the_newest_overflow() {
    let tabs = candidates(36);
    let locked = enforced_free().over_limit(LimitKey::AgentTabs, &tabs);
    assert_eq!(locked.iter().collect::<Vec<_>>(), ["item-035"]);
    assert_eq!(tabs.len(), 36);
    let teams = candidates(7);
    let locked = enforced_free().over_limit(LimitKey::Teams, &teams);
    assert_eq!(locked.iter().collect::<Vec<_>>(), ["item-006"]);
}

#[test]
fn paid_or_dormant_gates_lock_nothing() {
    let items = candidates(100);
    for limit in [LimitKey::Workspaces, LimitKey::Teams, LimitKey::AgentTabs] {
        assert!(LimitGate::new(unlimited(), true)
            .over_limit(limit, &items)
            .is_empty());
        assert!(LimitGate::dormant().over_limit(limit, &items).is_empty());
        assert!(LimitGate::new(compiled_free(), false)
            .over_limit(limit, &items)
            .is_empty());
    }
}

#[test]
fn lock_set_is_stable_under_input_order_and_ties_break_by_id() {
    let mut items = candidates(9);
    items.push(LockCandidate {
        id: "item-tie".into(),
        created_at: 1_000,
        exempt: false,
    });
    let forward = enforced_free().over_limit(LimitKey::Teams, &items);
    items.reverse();
    let reversed = enforced_free().over_limit(LimitKey::Teams, &items);
    assert_eq!(forward, reversed);
    assert_eq!(forward.len(), 4);
    assert!(!forward.contains("item-000"));
    assert!(!forward.contains("item-tie"));
    assert!(forward.contains("item-008"));
}

#[test]
fn locked_refusal_leads_with_its_own_code() {
    let text = LockedRefusal {
        limit: LimitKey::Teams,
        item_id: "team_ab".into(),
    }
    .to_string();
    assert!(text.starts_with(&format!("{PLAN_LIMIT_LOCKED}: team_ab ")));
    assert!(text.ends_with("(limit: teams)"));
    assert_ne!(PLAN_LIMIT_LOCKED, FREE_LIMIT_REACHED);
}

#[test]
fn wall_deadline_expires_summary_admission_and_existing_locks() {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    let wall = Arc::new(AtomicU64::new(99));
    let clock = wall.clone();
    let gate = LimitGate::new(unlimited(), true)
        .with_deadline(Some(100), Arc::new(move || clock.load(Ordering::SeqCst)));
    assert_eq!(gate.summary().workspaces, None);
    assert!(gate.admit(LimitKey::Workspaces, 2).is_ok());
    assert!(gate
        .over_limit(LimitKey::Workspaces, &candidates(3))
        .is_empty());
    wall.store(100, Ordering::SeqCst);
    assert_eq!(gate.summary().workspaces, Some(2));
    assert!(gate.admit(LimitKey::Workspaces, 2).is_err());
    assert_eq!(
        gate.over_limit(LimitKey::Workspaces, &candidates(3)).len(),
        1
    );
    wall.store(99, Ordering::SeqCst);
    assert_eq!(gate.summary().workspaces, Some(2));
}

#[test]
fn tab_refusals_use_the_shared_limit_catalog() {
    let noun: String =
        serde_json::from_str(include_str!("../../../../../shared/tabLimitNoun.json")).unwrap();
    assert_eq!(noun, "tabs running at once");
    let create = LimitRefusal {
        limit: LimitKey::AgentTabs,
        count: 35,
        max: 35,
    }
    .to_string();
    let locked = LockedRefusal {
        limit: LimitKey::AgentTabs,
        item_id: "tab".into(),
    }
    .to_string();
    assert!(create.contains(&format!("35 {noun}")));
    assert!(locked.contains(&format!("limit of {noun}")));
}
