use serde::Serialize;

use super::prompts_plan::REGRESSION_MILESTONE_KIND;
use super::reflection::GROUP_CLOSE_MILESTONE_KIND;
use crate::plan::{AgentRef, Milestone, Plan, RegressionGateOverride};

use super::{PlanError, PlanStore};

pub const GROUP_REGRESSION_LINK_PREFIX: &str = "group:";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegressionGate {
    pub key: String,
    pub label: String,
}

/// Outcome of evaluating a task dispatch against a plan's pending gates.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RegressionDispatchDecision {
    /// No terminal group has pending advisory records.
    Clear,
    /// Dispatch proceeds while the listed terminal-group records remain open.
    Advisory {
        regression_gates: Vec<RegressionGate>,
        retrospectives: Vec<RegressionGate>,
    },
}

pub fn regression_link(group_key: &str) -> String {
    format!("{GROUP_REGRESSION_LINK_PREFIX}{group_key}")
}

fn group_label(plan: &Plan, key: &str) -> String {
    plan.task_groups
        .iter()
        .find(|g| g.key == key)
        .map(|g| g.label.clone())
        .unwrap_or_else(|| key.to_string())
}

// >=1 Done required so an all-cancelled group (shipped nothing) never gates.
pub fn group_completion(plan: &Plan, key: &str) -> Option<u64> {
    let members: Vec<&_> = plan
        .tasks
        .iter()
        .filter(|t| t.group.as_deref() == Some(key))
        .collect();
    if members.is_empty() {
        return None;
    }
    if !members.iter().all(|t| t.status.is_terminal()) {
        return None;
    }
    if !members
        .iter()
        .any(|t| t.status == crate::plan::TaskStatus::Done)
    {
        return None;
    }
    members.iter().map(|t| t.status_changed_at).max()
}

fn latest_regression_ts(plan: &Plan, key: &str) -> Option<u64> {
    let link = regression_link(key);
    plan.strides
        .iter()
        .filter(|m| m.kind == REGRESSION_MILESTONE_KIND && m.links.iter().any(|l| l == &link))
        .map(|m| m.ts)
        .max()
}

// Reopening a task bumps the close past the old record, re-arming the gate.
pub fn regression_satisfied(plan: &Plan, key: &str, completed_at: u64) -> bool {
    latest_regression_ts(plan, key).is_some_and(|ts| ts >= completed_at)
}

// A recorded waiver silences a gate only while it post-dates the group's close,
// mirroring regression_satisfied so re-closing a member re-arms past the waiver.
fn override_satisfied(plan: &Plan, key: &str, completed_at: u64) -> bool {
    plan.regression_gate_overrides
        .iter()
        .any(|o| o.ts >= completed_at && o.gates.iter().any(|g| g == key))
}

// The retrospective is a `group_close` stride and re-arms exactly like the regression record.
fn group_retrospective_satisfied(plan: &Plan, key: &str, completed_at: u64) -> bool {
    let link = regression_link(key);
    plan.strides.iter().any(|m| {
        m.kind == GROUP_CLOSE_MILESTONE_KIND
            && m.ts >= completed_at
            && m.links.iter().any(|l| l == &link)
    })
}

fn completed_groups(plan: &Plan) -> Vec<(&str, u64)> {
    let mut seen = std::collections::HashSet::new();
    plan.tasks
        .iter()
        .filter_map(|task| task.group.as_deref())
        .filter(|key| seen.insert(*key))
        .filter_map(|key| group_completion(plan, key).map(|completed_at| (key, completed_at)))
        .collect()
}

fn sorted_gates<'a>(plan: &Plan, keys: impl Iterator<Item = &'a str>) -> Vec<RegressionGate> {
    let mut out: Vec<RegressionGate> = keys
        .map(|key| RegressionGate {
            key: key.to_string(),
            label: group_label(plan, key),
        })
        .collect();
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

// The override-aware set remains useful for status, but neither set controls dispatch.
fn compute_gates(plan: &Plan, respect_override: bool) -> Vec<RegressionGate> {
    let groups = completed_groups(plan);
    let keys = groups
        .iter()
        .filter(|(key, completed_at)| !regression_satisfied(plan, key, *completed_at))
        .filter(|(key, completed_at)| {
            !(respect_override && override_satisfied(plan, key, *completed_at))
        })
        .map(|(key, _)| *key);
    sorted_gates(plan, keys)
}

/// Terminal groups whose optional retrospective stride is missing or predates
/// their latest close. Overrides do not alter the recordkeeping status.
pub fn pending_group_retrospectives(plan: &Plan) -> Vec<RegressionGate> {
    let groups = completed_groups(plan);
    let keys = groups
        .iter()
        .filter(|(key, completed_at)| !group_retrospective_satisfied(plan, key, *completed_at))
        .map(|(key, _)| *key);
    sorted_gates(plan, keys)
}

// Complete groups whose optional regression evidence record is missing or stale.
pub fn pending_regression_gates(plan: &Plan) -> Vec<RegressionGate> {
    compute_gates(plan, false)
}

// Unwaived pending QA records, used to identify dispatch advisories.
pub fn blocking_regression_gates(plan: &Plan) -> Vec<RegressionGate> {
    compute_gates(plan, true)
}

/// Decides whether terminal-group recordkeeping should accompany a dispatch.
/// Never mutates and never refuses dispatch.
pub fn regression_dispatch_advisory(
    regression_gates: &[RegressionGate],
    retrospectives: &[RegressionGate],
) -> RegressionDispatchDecision {
    if regression_gates.is_empty() && retrospectives.is_empty() {
        return RegressionDispatchDecision::Clear;
    }
    RegressionDispatchDecision::Advisory {
        regression_gates: regression_gates.to_vec(),
        retrospectives: retrospectives.to_vec(),
    }
}

pub fn regression_dispatch_decision(plan: &Plan) -> RegressionDispatchDecision {
    regression_dispatch_advisory(
        &pending_regression_gates(plan),
        &pending_group_retrospectives(plan),
    )
}

impl PlanStore {
    // One plan-level working point per group, not a stride scattered per task.
    pub fn record_group_regression(
        &self,
        plan_id: &str,
        group_key: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<RegressionGate, PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mid = self.mint_id("ms");
        let mut gate = None;
        self.with_plan(plan_id, now_ms, |p| {
            let group = p
                .task_groups
                .iter()
                .find(|g| g.key == group_key)
                .ok_or(PlanError::GroupNotFound)?;
            gate = Some(RegressionGate {
                key: group.key.clone(),
                label: group.label.clone(),
            });
            p.strides.push(Milestone {
                id: mid.clone(),
                from,
                kind: REGRESSION_MILESTONE_KIND.into(),
                text: note.into(),
                ts: now_ms,
                links: vec![regression_link(group_key)],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })?;
        gate.ok_or(PlanError::GroupNotFound)
    }

    /// Record an authorized Orchestrator waiver of pending regression gates.
    /// Requires a reason and owner authorization; appends to the audit trail and
    /// never clears a gate implicitly. `gates` defaults to the currently blocking
    /// set so a bare override names exactly what it waives. Returns the record.
    pub fn record_regression_override(
        &self,
        plan_id: &str,
        gates: Vec<String>,
        reason: &str,
        actor: AgentRef,
        now_ms: u64,
    ) -> Result<RegressionGateOverride, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::OverrideReasonRequired);
        }
        let mut record = None;
        self.with_plan(plan_id, now_ms, |p| {
            if let Some(owner) = p.owner.tab_id.as_deref() {
                let authorized = actor.tab_id.as_deref() == Some(owner);
                if !authorized {
                    return Err(PlanError::OverrideNotAuthorized);
                }
            }
            let gate_keys = if gates.is_empty() {
                blocking_regression_gates(p)
                    .into_iter()
                    .map(|g| g.key)
                    .collect::<Vec<_>>()
            } else {
                gates.clone()
            };
            let entry = RegressionGateOverride {
                actor: actor.clone(),
                reason: reason.to_string(),
                gates: gate_keys,
                ts: now_ms,
            };
            p.regression_gate_overrides.push(entry.clone());
            record = Some(entry);
            Ok(())
        })?;
        record.ok_or(PlanError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, TaskGroup, TaskStatus};
    use crate::service_plan::NewTask;
    use tempfile::TempDir;

    fn store_with_two_groups() -> (TempDir, PlanStore, String, Vec<String>) {
        let dir = TempDir::new().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Gate plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
        );
        plan.task_groups = vec![
            TaskGroup {
                key: "phase-1".into(),
                label: "Phase 1".into(),
                color: None,
            },
            TaskGroup {
                key: "phase-2".into(),
                label: "Phase 2".into(),
                color: None,
            },
        ];
        store.save(&mut plan, 1).unwrap();
        let ids = ["a", "b", "c"]
            .iter()
            .map(|n| {
                store
                    .add_task(
                        &plan.id,
                        NewTask {
                            name: (*n).into(),
                            kind: "implement".into(),
                            group: Some(if *n == "c" {
                                "phase-2".into()
                            } else {
                                "phase-1".into()
                            }),
                            ..Default::default()
                        },
                        2,
                    )
                    .unwrap()
            })
            .collect();
        (dir, store, plan.id, ids)
    }

    fn set_status(store: &PlanStore, pid: &str, tid: &str, status: TaskStatus, now: u64) {
        store
            .with_plan(pid, now, |p| {
                let t = p.tasks.iter_mut().find(|t| t.id == tid).unwrap();
                t.status = status.clone();
                t.status_changed_at = now;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn incomplete_group_never_fires() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        // ids[1] still pending in phase-1.
        let plan = store.get(&pid).unwrap();
        assert!(pending_regression_gates(&plan).is_empty());
    }

    #[test]
    fn last_task_close_fires_once_and_record_silences() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 20);

        let plan = store.get(&pid).unwrap();
        let gates = pending_regression_gates(&plan);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].key, "phase-1");
        assert_eq!(gates[0].label, "Phase 1");

        store
            .record_group_regression(
                &pid,
                "phase-1",
                "full suite green",
                AgentRef::from_tab("qa"),
                30,
            )
            .unwrap();
        let plan = store.get(&pid).unwrap();
        assert!(
            pending_regression_gates(&plan).is_empty(),
            "recording the regression run silences the gate"
        );
    }

    #[test]
    fn reopening_a_task_rearms_the_gate() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 20);
        store
            .record_group_regression(&pid, "phase-1", "green", AgentRef::from_tab("qa"), 30)
            .unwrap();
        assert!(pending_regression_gates(&store.get(&pid).unwrap()).is_empty());

        // Reopen then re-close a member after the regression run — new work, stale gate.
        set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
        assert!(
            pending_regression_gates(&store.get(&pid).unwrap()).is_empty(),
            "an open member means the group is incomplete, so no gate yet"
        );
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);

        let gates = pending_regression_gates(&store.get(&pid).unwrap());
        assert_eq!(
            gates.len(),
            1,
            "reclosing after the old record re-arms the gate"
        );
        assert_eq!(gates[0].key, "phase-1");
    }

    #[test]
    fn all_cancelled_group_has_nothing_to_gate() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Cancelled, 10);
        set_status(&store, &pid, &ids[1], TaskStatus::Cancelled, 20);
        assert!(pending_regression_gates(&store.get(&pid).unwrap()).is_empty());
    }

    #[test]
    fn record_rejects_unknown_group_and_empty_note() {
        let (_d, store, pid, _ids) = store_with_two_groups();
        assert!(matches!(
            store.record_group_regression(&pid, "phase-1", "  ", AgentRef::from_tab("qa"), 5),
            Err(PlanError::QaNoteRequired)
        ));
        assert!(matches!(
            store.record_group_regression(&pid, "nope", "green", AgentRef::from_tab("qa"), 5),
            Err(PlanError::GroupNotFound)
        ));
    }

    fn close_phase_1(store: &PlanStore, pid: &str, ids: &[String]) {
        set_status(store, pid, &ids[0], TaskStatus::Done, 10);
        set_status(store, pid, &ids[1], TaskStatus::Done, 20);
    }

    #[test]
    fn dispatch_clear_when_no_pending_gates() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        // phase-1 still has an open member, so nothing gates.
        let plan = store.get(&pid).unwrap();
        assert_eq!(
            regression_dispatch_decision(&plan),
            RegressionDispatchDecision::Clear
        );
    }

    #[test]
    fn dispatch_advisory_names_the_pending_gate() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        let plan = store.get(&pid).unwrap();
        match regression_dispatch_decision(&plan) {
            RegressionDispatchDecision::Advisory {
                regression_gates, ..
            } => {
                assert_eq!(regression_gates.len(), 1);
                assert_eq!(regression_gates[0].key, "phase-1");
            }
            other => panic!("expected advisory, got {other:?}"),
        }
    }

    #[test]
    fn decision_is_a_pure_advisory_over_plain_gate_inputs() {
        let gates = vec![RegressionGate {
            key: "phase-1".into(),
            label: "Phase 1".into(),
        }];
        let retrospectives = vec![RegressionGate {
            key: "phase-2".into(),
            label: "Phase 2".into(),
        }];

        assert_eq!(
            regression_dispatch_advisory(&gates, &retrospectives),
            RegressionDispatchDecision::Advisory {
                regression_gates: gates,
                retrospectives,
            }
        );
    }

    #[test]
    fn override_records_audit_without_hiding_the_dispatch_advisory() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        let entry = store
            .record_regression_override(
                &pid,
                vec![],
                "warm QA unavailable; shipping hotfix under owner sign-off",
                AgentRef::from_tab("orch"),
                30,
            )
            .unwrap();
        assert_eq!(entry.gates, vec!["phase-1".to_string()]);
        assert_eq!(entry.ts, 30);

        let plan = store.get(&pid).unwrap();
        assert!(
            blocking_regression_gates(&plan).is_empty(),
            "override drops the gate from the blocking set"
        );
        assert_eq!(
            pending_regression_gates(&plan).len(),
            1,
            "advisory set still shows the gate needs a real QA run"
        );
        assert_eq!(plan.regression_gate_overrides.len(), 1);
        store
            .close_group(&pid, "phase-1", "retro", AgentRef::from_tab("orch"), 31)
            .unwrap();
        match regression_dispatch_decision(&store.get(&pid).unwrap()) {
            RegressionDispatchDecision::Advisory {
                regression_gates, ..
            } => assert_eq!(regression_gates[0].key, "phase-1"),
            other => panic!("expected advisory, got {other:?}"),
        }
    }

    #[test]
    fn override_requires_reason_and_owner() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        assert!(matches!(
            store.record_regression_override(&pid, vec![], "  ", AgentRef::from_tab("orch"), 30),
            Err(PlanError::OverrideReasonRequired)
        ));
        assert!(matches!(
            store.record_regression_override(
                &pid,
                vec![],
                "sneaky bypass",
                AgentRef::from_tab("worker"),
                30
            ),
            Err(PlanError::OverrideNotAuthorized)
        ));
        // Nothing was recorded by the rejected attempts.
        assert!(store
            .get(&pid)
            .unwrap()
            .regression_gate_overrides
            .is_empty());
    }

    #[test]
    fn override_rearms_when_group_recloses_after_the_waiver() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        store
            .record_regression_override(
                &pid,
                vec![],
                "owner waiver",
                AgentRef::from_tab("orch"),
                30,
            )
            .unwrap();
        assert!(blocking_regression_gates(&store.get(&pid).unwrap()).is_empty());

        // New work reopens then recloses a member past the waiver — gate re-arms.
        set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);
        let plan = store.get(&pid).unwrap();
        let blocking = blocking_regression_gates(&plan);
        assert_eq!(blocking.len(), 1, "stale waiver no longer covers new close");
        assert_eq!(blocking[0].key, "phase-1");
    }

    #[test]
    fn clearing_one_group_leaves_the_other_blocking() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        set_status(&store, &pid, &ids[2], TaskStatus::Done, 25); // phase-2 closes
        let plan = store.get(&pid).unwrap();
        assert_eq!(blocking_regression_gates(&plan).len(), 2);

        store
            .record_group_regression(&pid, "phase-1", "green", AgentRef::from_tab("qa"), 30)
            .unwrap();
        let plan = store.get(&pid).unwrap();
        let blocking = blocking_regression_gates(&plan);
        assert_eq!(blocking.len(), 1, "phase-2 still gates dispatch");
        assert_eq!(blocking[0].key, "phase-2");
    }

    fn retrospective_keys(store: &PlanStore, pid: &str) -> Vec<String> {
        pending_group_retrospectives(&store.get(pid).unwrap())
            .into_iter()
            .map(|gate| gate.key)
            .collect()
    }

    fn record_retrospective(store: &PlanStore, pid: &str, now: u64) {
        store
            .close_group(pid, "phase-1", "retro", AgentRef::from_tab("orch"), now)
            .unwrap();
    }

    #[test]
    fn milestone_retrospective_contract_terminal_group_gates_next_frontier() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        store
            .record_group_regression(&pid, "phase-1", "green", AgentRef::from_tab("qa"), 30)
            .unwrap();
        let plan = store.get(&pid).unwrap();
        assert!(blocking_regression_gates(&plan).is_empty());
        match regression_dispatch_decision(&plan) {
            RegressionDispatchDecision::Advisory {
                regression_gates,
                retrospectives,
            } => {
                assert!(regression_gates.is_empty());
                assert_eq!(retrospectives.len(), 1);
                assert_eq!(retrospectives[0].key, "phase-1");
            }
            other => panic!("expected retrospective advisory, got {other:?}"),
        }

        record_retrospective(&store, &pid, 40);
        let plan = store.get(&pid).unwrap();
        assert!(retrospective_keys(&store, &pid).is_empty());
        let retro = plan
            .strides
            .iter()
            .find(|m| m.kind == GROUP_CLOSE_MILESTONE_KIND)
            .unwrap();
        assert_eq!(retro.links, vec![regression_link("phase-1")]);
        assert_eq!(
            regression_dispatch_decision(&plan),
            RegressionDispatchDecision::Clear
        );
    }

    #[test]
    fn milestone_retrospective_contract_reclose_rearms_the_retrospective() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        record_retrospective(&store, &pid, 30);
        assert!(retrospective_keys(&store, &pid).is_empty());

        set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
        assert!(
            retrospective_keys(&store, &pid).is_empty(),
            "an open member means the group is not terminal"
        );
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);
        assert_eq!(
            retrospective_keys(&store, &pid),
            vec!["phase-1".to_string()]
        );
    }

    #[test]
    fn milestone_retrospective_contract_early_close_does_not_cover_later_completion() {
        let (_d, store, pid, ids) = store_with_two_groups();
        record_retrospective(&store, &pid, 5);
        assert!(retrospective_keys(&store, &pid).is_empty());
        close_phase_1(&store, &pid, &ids);
        assert_eq!(
            retrospective_keys(&store, &pid),
            vec!["phase-1".to_string()]
        );
    }

    #[test]
    fn milestone_retrospective_contract_override_never_waives_it() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        store
            .record_regression_override(&pid, vec![], "waiver", AgentRef::from_tab("orch"), 30)
            .unwrap();
        let plan = store.get(&pid).unwrap();
        assert!(blocking_regression_gates(&plan).is_empty());
        assert!(matches!(
            regression_dispatch_decision(&plan),
            RegressionDispatchDecision::Advisory { ref retrospectives, .. } if retrospectives.len() == 1
        ));
    }

    #[test]
    fn milestone_retrospective_contract_skips_open_and_all_cancelled_groups() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        assert!(retrospective_keys(&store, &pid).is_empty());
        set_status(&store, &pid, &ids[0], TaskStatus::Cancelled, 11);
        set_status(&store, &pid, &ids[1], TaskStatus::Cancelled, 12);
        assert!(retrospective_keys(&store, &pid).is_empty());
    }
}
