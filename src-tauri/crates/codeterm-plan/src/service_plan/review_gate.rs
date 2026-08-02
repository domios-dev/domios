use serde::Serialize;

use super::mutations::plan_contains_id;
use super::{PlanError, PlanStore};
use crate::plan::{
    AgentRef, Milestone, Plan, ReviewAuditEntry, ReviewCapacityClass, ReviewCapacityGrant,
    ReviewEvidence, ReviewOverride, ReviewPolicy, ReviewStage, ReviewVerdict, Task, TaskStatus,
    DEFAULT_MAX_REVIEW_ROUNDS,
};

pub const REVIEW_MILESTONE_KIND: &str = "review";
pub const REVIEW_ESCALATION_MILESTONE_KIND: &str = "review_escalation";
pub const REVIEW_CREDIT_RECONCILE_MILESTONE_KIND: &str = "review_credit_reconcile";
pub const REVIEW_CAPACITY_GRANT_MILESTONE_KIND: &str = "review_capacity_grant";
const REPEATED_STRATEGY_MESSAGE: &str = "Stop iterating this strategy. Re-diagnose per `codeterm docs BOUNCE`. Consider a fresh independent reviewer with a distinct oracle, a decision gate, or model escalation; then continue autonomously.";

fn max_review_rounds() -> u32 {
    std::env::var("CODETERM_PLAN_REVIEW_MAX_ROUNDS")
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_REVIEW_ROUNDS)
}

/// A stable signature over the fields that define "what must be true to accept"
/// for a task. Locked into `Task.review_oracle` the first time it enters review.
pub fn oracle_signature(task: &Task) -> String {
    let mut criteria = task.acceptance_criteria.clone();
    criteria.sort();
    format!(
        "ac:{}|tp:{}",
        criteria.join(";"),
        task.test_plan.as_deref().unwrap_or("").trim()
    )
}

/// An "executable exact QA plan": both a non-empty test plan and at least one
/// acceptance criterion, so an evaluator has a concrete oracle to run.
pub fn has_exact_qa_evidence(task: &Task) -> bool {
    task.test_plan
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        && !task.acceptance_criteria.is_empty()
}

/// Independently recorded review evidence: at least one prior verdict already
/// attached to this task by a distinct reviewer pass.
pub fn has_independent_review_evidence(task: &Task) -> bool {
    !task.review_evidence.is_empty()
}

fn override_is_complete(o: &ReviewOverride) -> bool {
    !o.owner.trim().is_empty() && !o.reason.trim().is_empty() && !o.reproduction.trim().is_empty()
}

pub fn in_review_count(plan: &Plan) -> usize {
    plan.tasks
        .iter()
        .filter(|t| t.status == TaskStatus::InReview)
        .count()
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct ReviewGateStatus {
    pub wip_count: usize,
    pub wip_limit: u32,
    pub credit: i64,
    pub blocked: bool,
    pub reason: Option<String>,
}

/// Read-only: does review capacity/credit currently forbid dispatching new
/// ordinary implementation work? Never mutates.
pub fn review_gate_status(plan: &Plan) -> ReviewGateStatus {
    let wip_count = in_review_count(plan);
    let wip_limit = plan.review_policy.wip_limit;
    let credit = plan.review_policy.credit;
    // Debt policy: once the closure/entry credit ledger runs a full WIP limit
    // in the red, review throughput (not raw slot count) is the constraint.
    let credit_debt_exceeded = credit <= -(wip_limit as i64);
    let wip_exceeded = wip_count >= wip_limit as usize;
    let blocked = wip_exceeded || credit_debt_exceeded;
    let reason = blocked.then(|| {
        if wip_exceeded {
            format!(
                "review_wip_exceeded: {wip_count}/{wip_limit} tasks in_review; close a review before dispatching more implementation work"
            )
        } else {
            format!(
                "review_credit_exceeded: credit={credit} (limit=-{wip_limit}); implementation is outrunning review closures"
            )
        }
    });
    ReviewGateStatus {
        wip_count,
        wip_limit,
        credit,
        blocked,
        reason,
    }
}

pub fn clears_review_capacity(plan: &Plan, task: &Task) -> bool {
    plan.review_policy
        .capacity_grants
        .iter()
        .any(|grant| grant.task_id == task.id)
}

pub fn validate_dispatch_review_capacity(plan: &Plan, task: &Task) -> Result<(), PlanError> {
    let gate = review_gate_status(plan);
    if !gate.blocked {
        return Ok(());
    }
    let debt_only =
        gate.wip_count < gate.wip_limit as usize && gate.credit <= -(gate.wip_limit as i64);
    if debt_only && clears_review_capacity(plan, task) {
        return Ok(());
    }
    Err(PlanError::ReviewWipExceeded {
        wip_count: gate.wip_count,
        wip_limit: gate.wip_limit,
        credit: gate.credit,
    })
}

/// Validates entry into `in_review`: evidence, review-depth bound, and oracle
/// stability. Does NOT check WIP/credit — that gates ordinary dispatch
/// (Pending -> in_progress), not review entry, so a task already in flight can
/// always be submitted for review.
pub fn validate_enter_review(task: &Task) -> Result<(), PlanError> {
    if !has_exact_qa_evidence(task) && !has_independent_review_evidence(task) {
        return Err(PlanError::ReviewEvidenceRequired);
    }
    let rounds = max_review_rounds();
    if task.review_round >= rounds {
        match &task.review_override {
            Some(o) if override_is_complete(o) => {}
            Some(_) => return Err(PlanError::ReviewOverrideIncomplete),
            None => {
                return Err(PlanError::ReviewRoundExceeded {
                    round: task.review_round,
                    max_rounds: rounds,
                })
            }
        }
    }
    if task.review_round > 0 {
        if let Some(prev_oracle) = task.review_oracle.as_deref() {
            let current = oracle_signature(task);
            let has_override = task
                .review_override
                .as_ref()
                .is_some_and(override_is_complete);
            if prev_oracle != current && !has_override {
                return Err(PlanError::ReviewOracleScopeExpansion);
            }
        }
    }
    Ok(())
}

/// Fixes the acceptance oracle on first review entry or an authorized correction.
/// Consumes bounded-round overrides into the immutable audit log.
pub fn fix_oracle_and_consume_override(task: &mut Task, now_ms: u64) {
    let rounds = max_review_rounds();
    let current_oracle = oracle_signature(task);
    let authorized_correction = task.review_round > 0
        && task
            .review_override
            .as_ref()
            .is_some_and(override_is_complete)
        && task.review_oracle.as_deref() != Some(current_oracle.as_str());
    if task.review_oracle.is_none() || authorized_correction {
        task.review_oracle = Some(current_oracle);
    }
    if task.review_round >= rounds {
        if let Some(o) = task.review_override.take() {
            if override_is_complete(&o) {
                task.review_audit.push(ReviewAuditEntry {
                    id: crate::plan::ids::short_id("aud"),
                    owner: o.owner,
                    reason: o.reason,
                    reproduction: o.reproduction,
                    round: task.review_round,
                    at: now_ms,
                });
            } else {
                task.review_override = Some(o);
            }
        }
    }
}

/// Entering `in_review` spends one unit of review credit.
pub fn spend_review_credit(policy: &mut ReviewPolicy) {
    policy.credit -= 1;
}

/// Closing a review round (BOUNCE, ACCEPT, or a direct `close_task` on a task
/// that was in_review) earns back one unit of review credit.
pub fn earn_review_credit(policy: &mut ReviewPolicy) {
    policy.credit += 1;
}

fn review_milestone(
    id: String,
    from: AgentRef,
    verdict: &ReviewVerdict,
    note: &str,
    now_ms: u64,
) -> Milestone {
    Milestone {
        id,
        from,
        kind: REVIEW_MILESTONE_KIND.into(),
        text: format!("{verdict:?}: {note}"),
        ts: now_ms,
        links: vec![],
        attachments: vec![],
        loop_id: None,
        iteration: None,
        canvas: Default::default(),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StrategyWarning {
    pub consecutive_bounces: usize,
    pub review_round: u32,
    pub message: String,
}

pub fn strategy_warning(task: &Task) -> Option<StrategyWarning> {
    let consecutive_bounces = task
        .review_evidence
        .iter()
        .rev()
        .take_while(|evidence| {
            evidence.verdict == ReviewVerdict::Bounce
                && !evidence.changed_executable_surfaces
                && !evidence.distinct_oracle
        })
        .count();
    (consecutive_bounces >= 3).then(|| StrategyWarning {
        consecutive_bounces,
        review_round: task.review_round,
        message: REPEATED_STRATEGY_MESSAGE.into(),
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewTransitionResult {
    pub plan_id: String,
    pub task_id: String,
    pub milestone_id: String,
    pub task_name: String,
    pub verdict: ReviewVerdict,
    pub review_round: u32,
    pub follow_up_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_warning: Option<StrategyWarning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ReviewContinuation>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewContinuation {
    pub attempt_id: String,
    pub executor_tab_id: String,
    pub session_id: String,
    pub epoch: u32,
    pub review_round: u32,
    pub state: String,
}

fn exact_review_continuation_attempt(task: &Task) -> Option<usize> {
    let candidates = task
        .execution_attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| {
            attempt.closed_at.is_none()
                && attempt.acknowledged_at.is_some()
                && attempt.reported_at.is_some()
                && attempt.session_id.as_deref().is_some_and(|value| !value.trim().is_empty())
                && if attempt.competition_group_id.is_some() {
                    attempt.winner
                } else {
                    task.assigned_tab_id.as_deref() == Some(attempt.executor_tab_id.as_str())
                }
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = candidates.as_slice() else {
        return None;
    };
    Some(*index)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewCreditReconcileResult {
    pub plan_id: String,
    pub previous_credit: i64,
    pub reconciled_credit: i64,
    pub active_reviews: usize,
    pub stride_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ReviewCapacityGrantResult {
    pub plan_id: String,
    pub grant: ReviewCapacityGrant,
    pub created: bool,
    pub stride_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QaWave {
    pub wave_id: String,
    pub group_key: String,
    pub label: String,
    pub task_ids: Vec<String>,
}

/// Groups batch-compatible (same task `group`) queued/running QA-stage tasks
/// into one named wave, mirroring the group-key dedup already used for
/// regression gates. Read-only: callers decide whether to claim/dispatch.
pub fn qa_waves(plan: &Plan) -> Vec<QaWave> {
    let mut by_group: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for task in &plan.tasks {
        if task.status != TaskStatus::InReview || task.review_stage != ReviewStage::Qa {
            continue;
        }
        let Some(key) = task.group.as_deref() else {
            continue;
        };
        by_group
            .entry(key.to_string())
            .or_default()
            .push(task.id.clone());
    }
    by_group
        .into_iter()
        .map(|(key, task_ids)| {
            let label = plan
                .task_groups
                .iter()
                .find(|g| g.key == key)
                .map(|g| g.label.clone())
                .unwrap_or_else(|| key.clone());
            QaWave {
                wave_id: format!("wave:{key}"),
                group_key: key,
                label,
                task_ids,
            }
        })
        .collect()
}

fn require_in_review(task: &Task) -> Result<(), PlanError> {
    if task.status == TaskStatus::InReview {
        Ok(())
    } else {
        Err(PlanError::ReviewNotInReview)
    }
}

impl PlanStore {
    /// Read-only review-backpressure status for heartbeat/status surfaces.
    pub fn review_gate_status(&self, plan_id: &str) -> Result<ReviewGateStatus, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        Ok(review_gate_status(&plan))
    }

    pub fn qa_waves(&self, plan_id: &str) -> Result<Vec<QaWave>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        Ok(qa_waves(&plan))
    }

    /// Authorizes the next over-round-limit re-entry into review. Consumed
    /// atomically (moved into the immutable audit log) the next time this
    /// task re-enters `in_review` past the round bound.
    pub fn record_review_override(
        &self,
        plan_id: &str,
        task_id: &str,
        owner: &str,
        reason: &str,
        reproduction: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let owner = owner.trim();
        let reason = reason.trim();
        let reproduction = reproduction.trim();
        if owner.is_empty() || reason.is_empty() || reproduction.is_empty() {
            return Err(PlanError::ReviewOverrideIncomplete);
        }
        self.with_plan(plan_id, now_ms, |p| {
            let task = p
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            task.review_override = Some(ReviewOverride {
                owner: owner.into(),
                reason: reason.into(),
                reproduction: reproduction.into(),
                at: now_ms,
            });
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn reconcile_review_credit(
        &self,
        plan_id: &str,
        reason: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<ReviewCreditReconcileResult, PlanError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mut result = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let active_reviews = in_review_count(plan);
            let previous_credit = plan.review_policy.credit;
            let reconciled_credit = previous_credit.max(-(active_reviews as i64));
            if reconciled_credit == previous_credit {
                result = Some(ReviewCreditReconcileResult {
                    plan_id: plan.id.clone(),
                    previous_credit,
                    reconciled_credit,
                    active_reviews,
                    stride_id: None,
                });
                return Ok(());
            }
            let stride_id = crate::plan::ids::mint_id("ms", |candidate| {
                plan_contains_id(plan, candidate)
            });
            plan.review_policy.credit = reconciled_credit;
            plan.strides.push(Milestone {
                id: stride_id.clone(),
                from,
                kind: REVIEW_CREDIT_RECONCILE_MILESTONE_KIND.into(),
                text: format!(
                    "{reason}; review credit reconciled {previous_credit} -> {reconciled_credit} with {active_reviews} active review(s)"
                ),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            result = Some(ReviewCreditReconcileResult {
                plan_id: plan.id.clone(),
                previous_credit,
                reconciled_credit,
                active_reviews,
                stride_id: Some(stride_id),
            });
            Ok(())
        })?;
        result.ok_or(PlanError::NotFound)
    }

    pub fn grant_review_capacity(
        &self,
        plan_id: &str,
        task_id: &str,
        class: ReviewCapacityClass,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<ReviewCapacityGrantResult, PlanError> {
        let mut result = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let canonical_task_id = plan
                .tasks
                .iter()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
                .map(|task| task.id.clone())
                .ok_or(PlanError::TaskNotFound)?;
            if let Some(existing) = plan
                .review_policy
                .capacity_grants
                .iter()
                .find(|grant| grant.task_id == canonical_task_id)
                .cloned()
            {
                if existing.class != class {
                    return Err(PlanError::Io(
                        "review capacity class is immutable once granted".into(),
                    ));
                }
                result = Some(ReviewCapacityGrantResult {
                    plan_id: plan.id.clone(),
                    grant: existing,
                    created: false,
                    stride_id: None,
                });
                return Ok(());
            }
            let stride_id =
                crate::plan::ids::mint_id("ms", |candidate| plan_contains_id(plan, candidate));
            let grant = ReviewCapacityGrant {
                task_id: canonical_task_id,
                class,
                granted_by: from.clone(),
                granted_at: now_ms,
            };
            plan.review_policy.capacity_grants.push(grant.clone());
            plan.strides.push(Milestone {
                id: stride_id.clone(),
                from,
                kind: REVIEW_CAPACITY_GRANT_MILESTONE_KIND.into(),
                text: format!(
                    "authorized {:?} review-capacity provenance for {}",
                    class, grant.task_id
                ),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            result = Some(ReviewCapacityGrantResult {
                plan_id: plan.id.clone(),
                grant,
                created: true,
                stride_id: Some(stride_id),
            });
            Ok(())
        })?;
        result.ok_or(PlanError::NotFound)
    }

    /// BOUNCE records an exact-attempt continuation epoch before delivery.
    /// The task remains in review until authenticated continuation succeeds.
    pub fn bounce_review(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        changed_executable_surfaces: bool,
        distinct_oracle: bool,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<ReviewTransitionResult, PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mut result = None;
        self.with_plan(plan_id, now_ms, |p| {
            let idx = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            if matches!(p.tasks[idx].status, TaskStatus::InReview | TaskStatus::InProgress)
                && matches!(
                    p.tasks[idx].status_label.as_deref(),
                    Some(
                        "awaiting_continuation"
                            | "bounce_continuation_failed"
                            | "review bounced"
                    )
                )
            {
                let attempt_index = exact_review_continuation_attempt(&p.tasks[idx])
                    .ok_or(PlanError::TaskNotFound)?;
                let attempt = &p.tasks[idx].execution_attempts[attempt_index];
                if attempt.continuation.review_round != Some(p.tasks[idx].review_round) {
                    return Err(PlanError::TaskNotFound);
                }
                result = Some(ReviewTransitionResult {
                    plan_id: p.id.clone(),
                    task_id: p.tasks[idx].id.clone(),
                    milestone_id: attempt
                        .continuation.milestone_id
                        .clone()
                        .ok_or(PlanError::TaskNotFound)?,
                    task_name: p.tasks[idx].name.clone(),
                    verdict: ReviewVerdict::Bounce,
                    review_round: p.tasks[idx].review_round,
                    follow_up_task_id: None,
                    strategy_warning: strategy_warning(&p.tasks[idx]),
                    continuation: Some(ReviewContinuation {
                        attempt_id: attempt.id.clone(),
                        executor_tab_id: attempt.executor_tab_id.clone(),
                        session_id: attempt.session_id.clone().ok_or(PlanError::TaskNotFound)?,
                        epoch: attempt.continuation.epoch,
                        review_round: p.tasks[idx].review_round,
                        state: attempt
                            .continuation.state
                            .clone()
                            .unwrap_or_else(|| "awaiting_continuation".into()),
                    }),
                });
                return Ok(());
            }
            require_in_review(&p.tasks[idx])?;
            let attempt_index = exact_review_continuation_attempt(&p.tasks[idx])
                .ok_or(PlanError::TaskNotFound)?;
            let mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            {
                let task = &mut p.tasks[idx];
                task.review_evidence.push(ReviewEvidence {
                    id: crate::plan::ids::short_id("rev"),
                    from: from.clone(),
                    verdict: ReviewVerdict::Bounce,
                    note: note.into(),
                    at: now_ms,
                    changed_executable_surfaces,
                    distinct_oracle,
                });
                task.review_round += 1;
                let attempt = &mut task.execution_attempts[attempt_index];
                attempt.continuation.epoch += 1;
                attempt.continuation.review_round = Some(task.review_round);
                attempt.continuation.state = Some("awaiting_continuation".into());
                attempt.continuation.milestone_id = Some(mid.clone());
                attempt.continuation.delivered_at = None;
                task.status = TaskStatus::InReview;
                task.review_stage = ReviewStage::Review;
                task.qa_state = None;
                task.status_label = Some("awaiting_continuation".into());
                task.status_changed_at = now_ms;
                task.updated_at = now_ms;
                task.milestones.push(review_milestone(
                    mid.clone(),
                    from,
                    &ReviewVerdict::Bounce,
                    note,
                    now_ms,
                ));
            }
            earn_review_credit(&mut p.review_policy);
            result = Some(ReviewTransitionResult {
                plan_id: p.id.clone(),
                task_id: p.tasks[idx].id.clone(),
                milestone_id: mid,
                task_name: p.tasks[idx].name.clone(),
                verdict: ReviewVerdict::Bounce,
                review_round: p.tasks[idx].review_round,
                follow_up_task_id: None,
                strategy_warning: strategy_warning(&p.tasks[idx]),
                continuation: p.tasks[idx]
                    .execution_attempts
                    .get(attempt_index)
                    .map(|attempt| ReviewContinuation {
                        attempt_id: attempt.id.clone(),
                        executor_tab_id: attempt.executor_tab_id.clone(),
                        session_id: attempt.session_id.clone().unwrap_or_default(),
                        epoch: attempt.continuation.epoch,
                        review_round: p.tasks[idx].review_round,
                        state: "awaiting_continuation".into(),
                    }),
            });
            Ok(())
        })?;
        result.ok_or(PlanError::TaskNotFound)
    }

    pub fn claim_review_continuation_delivery(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        epoch: u32,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let Some(attempt) = task.execution_attempts.iter_mut().find(|attempt| {
                attempt.id == attempt_id
                    && attempt.closed_at.is_none()
                    && attempt.continuation.epoch == epoch
            }) else {
                return Ok(false);
            };
            if task.status != TaskStatus::InReview
                || task.status_label.as_deref() != Some("awaiting_continuation")
                || attempt.continuation.state.as_deref() != Some("awaiting_continuation")
            {
                return Ok(false);
            }
            attempt.continuation.state = Some("continuation_delivering".into());
            task.updated_at = now_ms;
            Ok(true)
        })
    }

    pub fn finish_review_continuation(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        epoch: u32,
        delivered: bool,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let Some(attempt) = task.execution_attempts.iter_mut().find(|attempt| {
                attempt.id == attempt_id
                    && attempt.closed_at.is_none()
                    && attempt.continuation.epoch == epoch
                    && attempt.continuation.state.as_deref() == Some("continuation_delivering")
            }) else {
                return Ok(false);
            };
            if delivered {
                attempt.continuation.state = Some("continued".into());
                attempt.continuation.delivered_at = Some(now_ms);
                task.status = TaskStatus::InProgress;
                task.status_label = Some("review bounced".into());
                task.status_changed_at = now_ms;
            } else {
                attempt.continuation.state = Some("bounce_continuation_failed".into());
                task.status = TaskStatus::InReview;
                task.status_label = Some("bounce_continuation_failed".into());
            }
            task.updated_at = now_ms;
            Ok(true)
        })
    }

    /// ACCEPT verdict: attaches evidence AND closes the task (in_review -> done)
    /// in one step, so a reviewer never has to follow up with a separate
    /// `close_task`. Rejects while subtasks are still open. Past the default
    /// round bound, residual risk is routed to a separate follow-up task.
    pub fn accept_review(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        changed_executable_surfaces: bool,
        distinct_oracle: bool,
        residual_risk: Option<&str>,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<ReviewTransitionResult, PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mut result = None;
        self.with_plan(plan_id, now_ms, |p| {
            let idx = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            require_in_review(&p.tasks[idx])?;
            let mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            let rounds = max_review_rounds();
            let mut follow_up_task_id = None;
            {
                let task = &mut p.tasks[idx];
                task.review_evidence.push(ReviewEvidence {
                    id: crate::plan::ids::short_id("rev"),
                    from: from.clone(),
                    verdict: ReviewVerdict::Accept,
                    note: note.into(),
                    at: now_ms,
                    changed_executable_surfaces,
                    distinct_oracle,
                });
                task.updated_at = now_ms;
                task.milestones.push(review_milestone(
                    mid.clone(),
                    from.clone(),
                    &ReviewVerdict::Accept,
                    note,
                    now_ms,
                ));
            }
            super::mutations::finalize_task_done(p, idx, now_ms)?;
            if p.tasks[idx].review_round >= rounds {
                if let Some(risk) = residual_risk.map(str::trim).filter(|r| !r.is_empty()) {
                    let fid = crate::plan::ids::mint_id("task", |c| plan_contains_id(p, c));
                    let parent_id = p.tasks[idx].id.clone();
                    let parent_name = p.tasks[idx].name.clone();
                    let mut follow_up = Task {
                        id: fid.clone(),
                        name: format!("Follow-up hardening: {parent_name}"),
                        kind: "implement".into(),
                        model: None,
                        model_rationale: None,
                        personality: None,
                        personality_provenance: None,
                        acceptance_criteria: vec![],
                        external_refs: vec![],
                        test_plan: None,
                        qa_plan: None,
                        correctness: None,
                        status: TaskStatus::Pending,
                        review_stage: ReviewStage::Review,
                        qa_state: None,
                        review_round: 0,
                        review_oracle: None,
                        review_evidence: vec![],
                        review_override: None,
                        review_audit: vec![],
                        status_label: Some("residual risk from bounded review".into()),
                        priority: p.tasks[idx].priority.clone(),
                        created_at: now_ms,
                        updated_at: now_ms,
                        status_changed_at: now_ms,
                        handle: None,
                        tags: vec![],
                        section_ids: p.tasks[idx].section_ids.clone(),
                        body: format!("Residual risk documented at round bound: {risk}"),
                        role_guidance: None,
                        executor_role: p.tasks[idx].executor_role.clone(),
                        competition: None,
                        intent: None,
                        execution_intent: p.tasks[idx].execution_intent,
                        parent_task_id: Some(parent_id),
                        depends_on: vec![],
                        assigned_tab_id: None,
                        lane: Default::default(),
                        taken_by_role: None,
                        peer: None,
                        group: p.tasks[idx].group.clone(),
                        canvas: Default::default(),
                        milestones: vec![],
                        execution_attempts: vec![],
                        pause_reason: None,
                        attachments: vec![],
                    };
                    follow_up.id = fid.clone();
                    p.tasks.push(follow_up);
                    follow_up_task_id = Some(fid);
                }
            }
            earn_review_credit(&mut p.review_policy);
            result = Some(ReviewTransitionResult {
                plan_id: p.id.clone(),
                task_id: p.tasks[idx].id.clone(),
                milestone_id: mid,
                task_name: p.tasks[idx].name.clone(),
                verdict: ReviewVerdict::Accept,
                review_round: p.tasks[idx].review_round,
                follow_up_task_id,
                strategy_warning: None,
                continuation: None,
            });
            Ok(())
        })?;
        result.ok_or(PlanError::TaskNotFound)
    }

    /// Idempotent stale-review escalation: records a durable, auditable
    /// milestone once per stale spell (re-arms if the task re-enters review
    /// and goes stale again). Returns whether this call newly escalated it.
    pub fn escalate_stale_review(
        &self,
        plan_id: &str,
        task_id: &str,
        stale_after_ms: u64,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<bool, PlanError> {
        let mut escalated = false;
        self.with_plan(plan_id, now_ms, |p| {
            let idx = p
                .tasks
                .iter()
                .position(|t| t.id == task_id || t.handle.as_deref() == Some(task_id))
                .ok_or(PlanError::TaskNotFound)?;
            let task = &p.tasks[idx];
            require_in_review(task)?;
            let age_ms = now_ms.saturating_sub(task.status_changed_at);
            if age_ms <= stale_after_ms {
                return Ok(());
            }
            let already = task.milestones.iter().any(|m| {
                m.kind == REVIEW_ESCALATION_MILESTONE_KIND && m.ts >= task.status_changed_at
            });
            if already {
                return Ok(());
            }
            let mid = crate::plan::ids::mint_id("ms", |c| plan_contains_id(p, c));
            p.tasks[idx].milestones.push(Milestone {
                id: mid,
                from,
                kind: REVIEW_ESCALATION_MILESTONE_KIND.into(),
                text: format!(
                    "stale-review escalation: in_review for {age_ms}ms (threshold {stale_after_ms}ms)"
                ),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            escalated = true;
            Ok(())
        })?;
        Ok(escalated)
    }
}

#[cfg(test)]
mod tests;
