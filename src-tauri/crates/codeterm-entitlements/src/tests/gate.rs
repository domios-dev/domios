use crate::activation::{Activation, Phase, ACTIVATION};
use crate::gate::{admit, enforcing, LimitKey, LimitRefusal, FREE_LIMIT_REACHED};
use crate::payload::{Authority, Enforcement, Mode, Perks, Scope, ScopedLimit};
use crate::verdict::{compiled_free, EffectiveAccess};

fn activation(phase: Phase, enabled: bool) -> Activation {
    Activation {
        phase,
        project: "demo-domios",
        mode: Mode::Test,
        policy_version: 2,
        policy_epoch: "fixture",
        enforcement: Enforcement {
            enabled,
            rollback: false,
        },
        keys: &[],
    }
}

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
fn this_build_ships_dormant() {
    assert_eq!(ACTIVATION, None);
    assert!(!enforcing(ACTIVATION.as_ref()));
}

#[test]
fn only_phase_b_with_enforcement_on_enforces() {
    assert!(!enforcing(None));
    assert!(!enforcing(Some(&activation(Phase::A, true))));
    assert!(!enforcing(Some(&activation(Phase::B, false))));
    assert!(enforcing(Some(&activation(Phase::B, true))));
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
    assert_eq!(LimitKey::AgentTabs.key(), "agent.tabs");
}
