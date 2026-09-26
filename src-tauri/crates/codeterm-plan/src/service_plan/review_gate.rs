use serde::Serialize;
use sha2::{Digest, Sha256};

use super::mutations::plan_contains_id;
use super::{PlanError, PlanStore};
use crate::plan::{
    AgentRef, Milestone, Plan, PlanEvidenceClass, PlanEvidenceReporterGrant, ReviewCapacityClass,
    ReviewCapacityGrant, ReviewEvidence, ReviewOverride, ReviewPolicy, ReviewStage, ReviewVerdict,
    Task, TaskStatus, DEFAULT_MAX_REVIEW_ROUNDS,
};

mod bounce;

pub const REVIEW_MILESTONE_KIND: &str = "review";
pub const REVIEW_ESCALATION_MILESTONE_KIND: &str = "review_escalation";
pub const REVIEW_CREDIT_RECONCILE_MILESTONE_KIND: &str = "review_credit_reconcile";
pub const REVIEW_CAPACITY_GRANT_MILESTONE_KIND: &str = "review_capacity_grant";
pub const EVIDENCE_REPORTER_GRANT_MILESTONE_KIND: &str = "evidence_reporter_grant";
pub const PLAN_EVIDENCE_REPORT_MILESTONE_KIND: &str = "plan_evidence_report";
const ROUND_BOUND_STRATEGY_ADVICE: &str = "Round bound passed — this bounce and the Worker's next report are accepted, so treat it as advice, not a gate. Change strategy rather than re-running it: grant the Worker authority to run its own focused tests, take part of the work directly, or dispatch a second Worker with a distinct oracle.";
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

/// Review backlog is advisory; authorization and task lifecycle gates are separate.
pub fn review_gate_status(plan: &Plan) -> ReviewGateStatus {
    let wip_count = in_review_count(plan);
    let wip_limit = plan.review_policy.wip_limit;
    let credit = plan.review_policy.credit;
    // Debt policy: once the closure/entry credit ledger runs a full WIP limit
    // in the red, review throughput (not raw slot count) is the constraint.
    let credit_debt_exceeded = credit <= -(wip_limit as i64);
    let wip_exceeded = wip_count >= wip_limit as usize;
    let reason = (wip_exceeded || credit_debt_exceeded).then(|| {
        if wip_exceeded {
            format!(
                "review_backlog_warning: {wip_count}/{wip_limit} tasks in_review; prioritize closing reviews and avoid growing the backlog. Dispatch remains allowed."
            )
        } else {
            format!(
                "review_debt_warning: credit={credit} (threshold=-{wip_limit}); prioritize review closures. Dispatch remains allowed."
            )
        }
    });
    ReviewGateStatus {
        wip_count,
        wip_limit,
        credit,
        blocked: false,
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
    if let Some(reason) = gate.reason {
        log::warn!("[review-backlog] task={}: {}", task.id, reason);
    }
    Ok(())
}

/// A BOUNCE recorded with `distinct_oracle` is the reviewer stating that this
/// correction needs a different acceptance oracle. The correction answering it
/// is compliance, not expansion, so the scope gate must not claim it — otherwise
/// every compliant correction to such a bounce is refused as scope expansion and
/// the operator is moved onto the scope remedy for a round-domain event.
fn latest_bounce_reopened_oracle(task: &Task) -> bool {
    task.review_evidence
        .iter()
        .rev()
        .find(|evidence| evidence.verdict == ReviewVerdict::Bounce)
        .is_some_and(|evidence| evidence.distinct_oracle)
}

/// The only gate a review re-entry trips: an acceptance oracle that moved with
/// no bounce authorizing the change. Review depth is advisory, never a refusal.
fn oracle_scope_expanded(task: &Task) -> bool {
    if task.review_round == 0 || latest_bounce_reopened_oracle(task) {
        return false;
    }
    task.review_oracle
        .as_deref()
        .is_some_and(|prev_oracle| prev_oracle != oracle_signature(task))
}

/// Validates entry into `in_review`: evidence and oracle stability. Review
/// depth, WIP and credit are advisory and never prevent review entry.
pub fn validate_enter_review(task: &Task) -> Result<(), PlanError> {
    if !has_exact_qa_evidence(task) && !has_independent_review_evidence(task) {
        return Err(PlanError::ReviewEvidenceRequired);
    }
    if !oracle_scope_expanded(task) {
        return Ok(());
    }
    match &task.review_override {
        Some(o) if override_is_complete(o) => Ok(()),
        Some(_) => Err(PlanError::ReviewOverrideIncomplete),
        None => Err(PlanError::ReviewOracleScopeExpansion),
    }
}

/// Fixes the acceptance oracle on first review entry or an authorized correction,
/// then retires the override it spent.
///
/// Nothing is written to `review_audit` any more: that log records review-ROUND-
/// LIMIT exceptions, and the round limit no longer refuses anything.
pub fn fix_oracle_and_consume_override(task: &mut Task, _now_ms: u64) {
    // Read before the oracle moves: the scope verdict depends on the stored one.
    let spent_on_scope = oracle_scope_expanded(task);
    let current_oracle = oracle_signature(task);
    let authorized_correction = task.review_round > 0
        && task.review_oracle.as_deref() != Some(current_oracle.as_str())
        && (latest_bounce_reopened_oracle(task)
            || task
                .review_override
                .as_ref()
                .is_some_and(override_is_complete));
    if task.review_oracle.is_none() || authorized_correction {
        task.review_oracle = Some(current_oracle);
    }
    if !spent_on_scope {
        return;
    }
    let Some(spent) = task.review_override.take() else {
        return;
    };
    if !override_is_complete(&spent) {
        task.review_override = Some(spent);
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

/// The milestone `kind` `codeterm plan record --step decision:<text>` writes.
/// Selection for the strategy warning below keys off this constant, never off
/// rendered message text, so the mechanism stays assertable independent of wording.
pub const DECISION_STRIDE_KIND: &str = "decision";

/// A recorded decision, carried structurally (id/text/ts) rather than folded
/// into the warning's prose so a caller can re-read it without parsing text.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DecisionStride {
    pub id: String,
    pub text: String,
    pub ts: u64,
}

fn task_decision_strides(task: &Task) -> Vec<DecisionStride> {
    task.milestones
        .iter()
        .filter(|m| m.kind == DECISION_STRIDE_KIND)
        .map(|m| DecisionStride {
            id: m.id.clone(),
            text: m.text.clone(),
            ts: m.ts,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StrategyWarning {
    pub consecutive_bounces: usize,
    pub review_round: u32,
    pub message: String,
    /// Set when this advisory was raised because the round bound was passed, so
    /// a caller reads the condition as a flag instead of parsing the message.
    pub round_bound_advisory: bool,
    /// The task's recorded decisions, so the moment most likely to need them
    /// (a stuck strategy) is the moment they're re-surfaced. Empty when none
    /// were ever recorded — the warning still fires; it never gets suppressed.
    pub decisions: Vec<DecisionStride>,
}

fn consecutive_stagnant_bounces(task: &Task) -> usize {
    task.review_evidence
        .iter()
        .rev()
        .take_while(|evidence| {
            evidence.verdict == ReviewVerdict::Bounce
                && !evidence.changed_executable_surfaces
                && !evidence.distinct_oracle
        })
        .count()
}

pub fn strategy_warning(task: &Task) -> Option<StrategyWarning> {
    let consecutive_bounces = consecutive_stagnant_bounces(task);
    (consecutive_bounces >= 3).then(|| StrategyWarning {
        consecutive_bounces,
        review_round: task.review_round,
        message: REPEATED_STRATEGY_MESSAGE.into(),
        round_bound_advisory: false,
        decisions: task_decision_strides(task),
    })
}

/// True once the task has bounced past the configured round bound — the third
/// bounce under the default of two rounds.
pub fn past_round_bound(task: &Task) -> bool {
    task.review_round > max_review_rounds()
}

/// The advisory a recorded bounce carries back to the Orchestrator. Past the
/// round bound the strategy-change advice leads; a stagnant streak keeps its own
/// re-diagnosis guidance appended rather than replaced.
pub fn bounce_strategy_advice(task: &Task) -> Option<StrategyWarning> {
    let stagnant = strategy_warning(task);
    if !past_round_bound(task) {
        return stagnant;
    }
    let message = match &stagnant {
        Some(warning) => format!("{ROUND_BOUND_STRATEGY_ADVICE} {}", warning.message),
        None => ROUND_BOUND_STRATEGY_ADVICE.to_string(),
    };
    Some(StrategyWarning {
        consecutive_bounces: consecutive_stagnant_bounces(task),
        review_round: task.review_round,
        message,
        round_bound_advisory: true,
        decisions: task_decision_strides(task),
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
    #[serde(skip)]
    pub mesh_peer: Option<String>,
    pub epoch: u32,
    pub review_round: u32,
    pub state: String,
}

pub(crate) fn reset_completed_review_continuation(
    task: &mut Task,
    attempt_index: usize,
) -> Result<(), PlanError> {
    let Some(attempt) = task.execution_attempts.get_mut(attempt_index) else {
        return Err(PlanError::ReviewContinuationAttemptNotFound {
            task_id: task.id.clone(),
        });
    };
    if attempt.continuation.state.as_deref() != Some("continued") {
        return Ok(());
    }
    attempt.continuation.epoch = attempt
        .continuation
        .epoch
        .checked_add(1)
        .ok_or_else(|| PlanError::Io("review continuation epoch exhausted".into()))?;
    attempt.continuation.review_round = None;
    attempt.continuation.state = None;
    attempt.continuation.milestone_id = None;
    attempt.continuation.delivered_at = None;
    task.status_label = None;
    Ok(())
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

pub fn plan_evidence_binding_digest(nonce: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(nonce.as_bytes()))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanEvidenceReporterGrantResult {
    pub plan_id: String,
    pub grant: PlanEvidenceReporterGrant,
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
            let task = crate::plan::task_key::find_task_mut(&mut p.tasks, task_id)
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

    #[allow(clippy::too_many_arguments)]
    pub fn grant_plan_evidence_reporter(
        &self,
        plan_id: &str,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        class: PlanEvidenceClass,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<PlanEvidenceReporterGrantResult, PlanError> {
        let mut result = None;
        let session_binding_digest = plan_evidence_binding_digest(session_binding_nonce);
        self.with_plan(plan_id, now_ms, |plan| {
            if let Some(existing) = plan
                .evidence_reporters
                .iter()
                .find(|grant| {
                    grant.tab_id == tab_id
                        && grant.session_id == session_id
                        && grant.session_binding_digest == session_binding_digest
                })
                .cloned()
            {
                if existing.class != class {
                    return Err(PlanError::Io(
                        "plan evidence class is immutable for an authorized identity".into(),
                    ));
                }
                result = Some(PlanEvidenceReporterGrantResult {
                    plan_id: plan.id.clone(),
                    grant: existing,
                    created: false,
                    stride_id: None,
                });
                return Ok(());
            }
            let stride_id =
                crate::plan::ids::mint_id("ms", |candidate| plan_contains_id(plan, candidate));
            let grant = PlanEvidenceReporterGrant {
                tab_id: tab_id.to_string(),
                session_id: session_id.to_string(),
                session_binding_digest,
                class,
                granted_by: from.clone(),
                granted_at: now_ms,
            };
            plan.evidence_reporters.push(grant.clone());
            plan.strides.push(Milestone {
                id: stride_id.clone(),
                from,
                kind: EVIDENCE_REPORTER_GRANT_MILESTONE_KIND.into(),
                text: format!("authorized {:?} plan evidence reporter {}", class, tab_id),
                ts: now_ms,
                links: vec![],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            result = Some(PlanEvidenceReporterGrantResult {
                plan_id: plan.id.clone(),
                grant,
                created: true,
                stride_id: Some(stride_id),
            });
            Ok(())
        })?;
        result.ok_or(PlanError::NotFound)
    }

    pub fn add_plan_evidence_report(
        &self,
        plan_id: &str,
        text: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        self.add_plan_stride(
            plan_id,
            PLAN_EVIDENCE_REPORT_MILESTONE_KIND,
            text,
            from,
            now_ms,
        )
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
            let canonical_task_id = crate::plan::task_key::find_task(&plan.tasks, task_id)
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

    pub fn reopen_review_continuation(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        epoch: u32,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let task = &mut plan.tasks[task_index];
            if !matches!(task.status, TaskStatus::InReview | TaskStatus::InProgress)
                || !matches!(
                    task.status_label.as_deref(),
                    Some("awaiting_continuation" | "bounce_continuation_failed")
                )
            {
                return Err(PlanError::ReviewContinuationAttemptNotFound {
                    task_id: task.id.clone(),
                });
            }
            let review_round = task.review_round;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .filter(|attempt| {
                    attempt.continuation.epoch == epoch
                        && attempt.continuation.review_round == Some(review_round)
                })
                .ok_or_else(|| PlanError::ReviewContinuationAttemptNotFound {
                    task_id: task.id.clone(),
                })?;
            attempt.closed_at.get_or_insert(now_ms);
            attempt
                .closure_reason
                .get_or_insert_with(|| "review_bounce_executor_gone".into());
            attempt.continuation.state = Some("executor_gone".into());
            task.status = TaskStatus::Pending;
            task.status_label = Some("review bounced".into());
            task.status_changed_at = now_ms;
            task.updated_at = now_ms;
            Ok(())
        })
    }

    pub fn hold_review(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<ReviewTransitionResult, PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mut result = None;
        self.with_plan(plan_id, now_ms, |plan| {
            let idx = crate::plan::task_key::find_task_index(&plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            require_in_review(&plan.tasks[idx])?;
            let milestone_id =
                crate::plan::ids::mint_id("ms", |candidate| plan_contains_id(plan, candidate));
            let task = &mut plan.tasks[idx];
            task.review_evidence.push(ReviewEvidence {
                id: crate::plan::ids::short_id("rev"),
                from: from.clone(),
                verdict: ReviewVerdict::Hold,
                note: note.into(),
                at: now_ms,
                changed_executable_surfaces: false,
                distinct_oracle: false,
            });
            task.status_label = Some("review held".into());
            task.updated_at = now_ms;
            task.milestones.push(review_milestone(
                milestone_id.clone(),
                from,
                &ReviewVerdict::Hold,
                note,
                now_ms,
            ));
            result = Some(ReviewTransitionResult {
                plan_id: plan.id.clone(),
                task_id: task.id.clone(),
                milestone_id,
                task_name: task.name.clone(),
                verdict: ReviewVerdict::Hold,
                review_round: task.review_round,
                follow_up_task_id: None,
                strategy_warning: None,
                continuation: None,
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
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task_index = plan
                .tasks
                .iter()
                .position(|task| task.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let task = &plan.tasks[task_index];
            let attempt = task
                .execution_attempts
                .iter()
                .find(|attempt| attempt.id == attempt_id);
            let valid = task.status == TaskStatus::InReview
                && task.status_label.as_deref() == Some("awaiting_continuation")
                && attempt.is_some_and(|attempt| {
                    attempt.closed_at.is_none()
                        && attempt.continuation.epoch == epoch
                        && attempt.continuation.state.as_deref() == Some("awaiting_continuation")
                });
            if !valid {
                return Err(PlanError::ReviewContinuationDeliveryRefused {
                    task_id: task.id.clone(),
                    attempt_id: attempt_id.into(),
                    expected_epoch: epoch,
                    actual_epoch: attempt.map(|attempt| attempt.continuation.epoch),
                    task_status: format!("{:?}", task.status),
                    task_status_label: task.status_label.clone(),
                    continuation_state: attempt
                        .and_then(|attempt| attempt.continuation.state.clone()),
                    attempt_closed: attempt.map(|attempt| attempt.closed_at.is_some()),
                });
            }
            let attempt_index = task
                .execution_attempts
                .iter()
                .position(|attempt| attempt.id == attempt_id)
                .expect("validated continuation attempt must still exist");
            let task = &mut plan.tasks[task_index];
            let attempt = &mut task.execution_attempts[attempt_index];
            attempt.continuation.state = Some("continuation_delivering".into());
            task.updated_at = now_ms;
            Ok(())
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
        let needs_follow_up = residual_risk.is_some_and(|risk| !risk.trim().is_empty())
            && self.get(plan_id).is_some_and(|plan| {
                crate::plan::task_key::find_task(&plan.tasks, task_id)
                    .is_some_and(|task| task.review_round >= max_review_rounds())
            });
        let mut follow_up_key = self
            .issue_task_keys(plan_id, usize::from(needs_follow_up))
            .into_iter()
            .next();
        self.with_plan(plan_id, now_ms, |p| {
            let idx = crate::plan::task_key::find_task_index(&p.tasks, task_id)
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
                        key: follow_up_key.take(),
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
                        interruption: Default::default(),
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
            let idx = crate::plan::task_key::find_task_index(&p.tasks, task_id)
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
