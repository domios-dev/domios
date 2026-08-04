use serde::Serialize;

use super::prompts_plan::REGRESSION_MILESTONE_KIND;
use crate::plan::{AgentRef, Milestone, Plan, RegressionGateOverride, Task};

use super::{PlanError, PlanStore};

pub const GROUP_REGRESSION_LINK_PREFIX: &str = "group:";

/// Task tag marking work whose purpose IS to run the regression/QA that clears
/// pending gates — it must stay dispatchable so the frontier can be unblocked.
pub const GATE_CLEARING_TAG: &str = "regression-gate";
/// Task tag marking work the existing policy classifies as release-blocking, so
/// it dispatches even while broad gates are pending.
pub const RELEASE_BLOCKING_TAG: &str = "release-blocking";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegressionGate {
    pub key: String,
    pub label: String,
}

/// Why a task escapes a pending regression gate without an explicit override.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchExemption {
    GateClearing,
    ReleaseBlocking,
}

impl DispatchExemption {
    pub fn reason(self) -> &'static str {
        match self {
            DispatchExemption::GateClearing => {
                "gate-clearing work (regression-gate tag): runs the QA that clears the gate"
            }
            DispatchExemption::ReleaseBlocking => {
                "release-blocking work (release-blocking tag): dispatchable by policy"
            }
        }
    }
}

/// Outcome of evaluating a task dispatch against a plan's pending gates.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RegressionDispatchDecision {
    /// No blocking gate stands in the way — ordinary dispatch semantics.
    Clear,
    /// A narrow, visible exception carries the task past the gate.
    Exempt {
        exemption: DispatchExemption,
        blocking: Vec<RegressionGate>,
    },
    /// Dispatch must be refused until these gates go green or are overridden.
    Blocked { blocking: Vec<RegressionGate> },
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

// Complete groups whose full-gate regression is missing or stale. `respect_override`
// drops gates cleared by an authorized waiver, yielding the set that actually blocks
// dispatch; without it the full advisory set (what still needs a real QA run) shows.
fn compute_gates(plan: &Plan, respect_override: bool) -> Vec<RegressionGate> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for task in &plan.tasks {
        let Some(key) = task.group.as_deref() else {
            continue;
        };
        if !seen.insert(key.to_string()) {
            continue;
        }
        let Some(completed_at) = group_completion(plan, key) else {
            continue;
        };
        if regression_satisfied(plan, key, completed_at) {
            continue;
        }
        if respect_override && override_satisfied(plan, key, completed_at) {
            continue;
        }
        out.push(RegressionGate {
            key: key.to_string(),
            label: group_label(plan, key),
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

// Advisory only — complete groups whose full-gate regression is missing or stale.
pub fn pending_regression_gates(plan: &Plan) -> Vec<RegressionGate> {
    compute_gates(plan, false)
}

// Enforcing set — pending gates minus those an authorized override has waived.
pub fn blocking_regression_gates(plan: &Plan) -> Vec<RegressionGate> {
    compute_gates(plan, true)
}

/// Which narrow, always-visible exception (if any) lets a task past a pending gate.
pub fn task_dispatch_exemption(task: &Task) -> Option<DispatchExemption> {
    let has = |tag: &str| task.tags.iter().any(|t| t.eq_ignore_ascii_case(tag));
    if has(GATE_CLEARING_TAG) {
        Some(DispatchExemption::GateClearing)
    } else if has(RELEASE_BLOCKING_TAG) {
        Some(DispatchExemption::ReleaseBlocking)
    } else {
        None
    }
}

/// Decide whether dispatching `task` is allowed given the plan's pending gates.
/// Never mutates. A blocked decision names every gate that stands in the way.
pub fn regression_dispatch_decision(plan: &Plan, task: &Task) -> RegressionDispatchDecision {
    let blocking = blocking_regression_gates(plan);
    if blocking.is_empty() {
        return RegressionDispatchDecision::Clear;
    }
    if let Some(exemption) = task_dispatch_exemption(task) {
        return RegressionDispatchDecision::Exempt {
            exemption,
            blocking,
        };
    }
    RegressionDispatchDecision::Blocked { blocking }
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

    fn candidate(tags: &[&str]) -> Task {
        serde_json::from_value(serde_json::json!({
            "id": "task_cand",
            "name": "Frontier work",
            "type": "implement",
            "model": null,
            "model_rationale": null,
            "status": "pending",
            "status_label": null,
            "section_ids": [],
            "body": "",
            "intent": null,
            "assigned_tab_id": null,
            "peer": null,
            "tags": tags,
        }))
        .expect("candidate task fixture")
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
            regression_dispatch_decision(&plan, &candidate(&[])),
            RegressionDispatchDecision::Clear
        );
    }

    #[test]
    fn dispatch_blocked_names_the_pending_gate() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        let plan = store.get(&pid).unwrap();
        match regression_dispatch_decision(&plan, &candidate(&[])) {
            RegressionDispatchDecision::Blocked { blocking } => {
                assert_eq!(blocking.len(), 1);
                assert_eq!(blocking[0].key, "phase-1");
            }
            other => panic!("expected Blocked, got {other:?}"),
        }
    }

    #[test]
    fn gate_clearing_and_release_blocking_tags_are_exempt() {
        let (_d, store, pid, ids) = store_with_two_groups();
        close_phase_1(&store, &pid, &ids);
        let plan = store.get(&pid).unwrap();
        for (tag, expected) in [
            (GATE_CLEARING_TAG, DispatchExemption::GateClearing),
            (RELEASE_BLOCKING_TAG, DispatchExemption::ReleaseBlocking),
        ] {
            match regression_dispatch_decision(&plan, &candidate(&[tag])) {
                RegressionDispatchDecision::Exempt {
                    exemption,
                    blocking,
                } => {
                    assert_eq!(exemption, expected);
                    assert_eq!(blocking[0].key, "phase-1", "exemption still names the gate");
                }
                other => panic!("expected Exempt for {tag}, got {other:?}"),
            }
        }
    }

    #[test]
    fn override_waives_gate_records_audit_and_clears_dispatch() {
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
        assert_eq!(
            regression_dispatch_decision(&plan, &candidate(&[])),
            RegressionDispatchDecision::Clear
        );
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
}
