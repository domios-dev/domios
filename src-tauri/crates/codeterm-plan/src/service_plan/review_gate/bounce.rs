use super::*;
use std::collections::HashSet;

#[cfg(test)]
mod cleaned_bounce_tests;

enum ClosedReviewAttempt {
    None,
    Unique(usize),
    Ambiguous,
}

fn review_attempt_matches_task(task: &Task, attempt: &crate::plan::TaskExecutionAttempt) -> bool {
    let assigned_executor =
        task.assigned_tab_id.as_deref() == Some(attempt.executor_tab_id.as_str());
    let executor_retired_on_tab_close = task.status == TaskStatus::InReview
        && task.status_label.as_deref() == Some(crate::plan::reconcile::TAB_GONE_LABEL)
        && task.assigned_tab_id.is_none()
        && attempt.retired_at.is_some();
    attempt.acknowledged_at.is_some()
        && attempt.reported_at.is_some()
        && attempt
            .session_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && if attempt.competition_group_id.is_some() {
            attempt.winner
        } else {
            assigned_executor || executor_retired_on_tab_close
        }
}

fn exact_review_continuation_attempt(task: &Task) -> Option<usize> {
    let candidates = task
        .execution_attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| {
            attempt.closed_at.is_none() && review_attempt_matches_task(task, attempt)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = candidates.as_slice() else {
        return None;
    };
    Some(*index)
}

fn exact_closed_review_attempt(task: &Task) -> ClosedReviewAttempt {
    let candidates = task
        .execution_attempts
        .iter()
        .enumerate()
        .filter(|(_, attempt)| {
            attempt.closed_at.is_some() && review_attempt_matches_task(task, attempt)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => ClosedReviewAttempt::None,
        [index] => ClosedReviewAttempt::Unique(*index),
        _ => ClosedReviewAttempt::Ambiguous,
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn has_competition_history(task: &Task) -> bool {
    task.competition
        .as_ref()
        .is_some_and(|competition| competition.enabled)
        || task.execution_attempts.iter().any(|attempt| {
            attempt.competition_group_id.is_some()
                || attempt.competition_lane.is_some()
                || attempt.judge_verdict.is_some()
                || attempt.judged_at.is_some()
                || attempt.winner
        })
}

fn continuation_matches_review(task: &Task, attempt: &crate::plan::TaskExecutionAttempt) -> bool {
    match (
        attempt.continuation.review_round,
        attempt.continuation.state.as_deref(),
        nonempty(attempt.continuation.milestone_id.as_deref()),
        attempt.continuation.delivered_at,
    ) {
        (None, None, None, None) => attempt.continuation.epoch == 0,
        (Some(round), Some("continued"), Some(milestone_id), Some(delivered_at)) => {
            round > 0
                && attempt.continuation.epoch > 0
                && round == task.review_round
                && task.milestones.iter().any(|milestone| {
                    milestone.id == milestone_id && milestone.kind == REVIEW_MILESTONE_KIND
                })
                && attempt
                    .assigned_at
                    .is_some_and(|assigned_at| assigned_at < delivered_at)
                && attempt
                    .reported_at
                    .is_some_and(|reported_at| delivered_at < reported_at)
        }
        _ => false,
    }
}

fn cleaned_terminal_review_attempt(task: &Task) -> Option<usize> {
    if task.status != TaskStatus::InReview
        || task.assigned_tab_id.is_some()
        || task.taken_by_role.is_some()
        || task.peer.is_some()
        || task.lane.tab_id.is_some()
        || task.lane.worktree_path.is_some()
        || task.lane.branch.is_some()
        || task.lane.workspace_id.is_some()
        || task.lane.workspace_name.is_some()
        || !task
            .lane
            .cleanup_status
            .as_deref()
            .is_some_and(|status| status.trim().eq_ignore_ascii_case("done"))
        || task.interruption.owner_tab_id.is_some()
        || task.interruption.owner_session_id.is_some()
        || task.interruption.handoff_required
        || matches!(
            task.status_label.as_deref(),
            Some("awaiting_continuation" | "bounce_continuation_failed" | "review bounced")
        )
        || task.execution_attempts.is_empty()
        || has_competition_history(task)
    {
        return None;
    }

    let mut attempt_ids = HashSet::new();
    let mut session_ids = HashSet::new();
    let mut report_ids = HashSet::new();
    let mut previous_assignment = None;
    let mut previous_closure = None;
    for attempt in &task.execution_attempts {
        let attempt_id = nonempty(Some(attempt.id.as_str()))?;
        nonempty(Some(attempt.executor_tab_id.as_str()))?;
        let assigned_at = attempt.assigned_at?;
        let closed_at = attempt.closed_at?;
        if !attempt_ids.insert(attempt_id)
            || previous_assignment.is_some_and(|previous| previous >= assigned_at)
            || previous_closure.is_some_and(|previous| previous > assigned_at)
            || closed_at <= assigned_at
            || attempt
                .acknowledged_at
                .is_some_and(|acknowledged_at| acknowledged_at <= assigned_at)
            || attempt.reported_at.is_some_and(|reported_at| {
                attempt
                    .acknowledged_at
                    .is_none_or(|acknowledged_at| reported_at <= acknowledged_at)
            })
            || attempt
                .acknowledged_at
                .is_some_and(|acknowledged_at| acknowledged_at >= closed_at)
            || attempt
                .reported_at
                .is_some_and(|reported_at| reported_at >= closed_at)
            || attempt.report.is_some() != attempt.reported_at.is_some()
            || attempt
                .retired_at
                .is_some_and(|retired_at| retired_at < closed_at)
            || attempt.mesh_peer.is_some()
            || attempt.remote_dispatch_state.is_some()
            || attempt.remote_worktree_path.is_some()
            || attempt.remote_lane_tip.is_some()
            || attempt.remote_merge_commit.is_some()
        {
            return None;
        }
        if let Some(session_id) = attempt.session_id.as_deref() {
            let session_id = nonempty(Some(session_id))?;
            if !session_ids.insert(session_id) {
                return None;
            }
        }
        if let Some(report) = attempt.report.as_ref() {
            let report_id = nonempty(Some(report.id.as_str()))?;
            if !report_ids.insert(report_id) {
                return None;
            }
        }
        previous_assignment = Some(assigned_at);
        previous_closure = Some(closed_at);
    }

    let index = task.execution_attempts.len() - 1;
    let terminal = &task.execution_attempts[index];
    let assigned_at = terminal.assigned_at?;
    let acknowledged_at = terminal.acknowledged_at?;
    let reported_at = terminal.reported_at?;
    let closed_at = terminal.closed_at?;
    let retired_at = terminal.retired_at?;
    nonempty(terminal.session_id.as_deref())?;
    nonempty(terminal.session_binding_nonce.as_deref())?;
    let report = terminal.report.as_ref()?;
    if assigned_at >= acknowledged_at
        || acknowledged_at >= reported_at
        || reported_at >= closed_at
        || closed_at > retired_at
        || nonempty(Some(report.id.as_str())).is_none()
        || report.status.as_deref() != Some("in_review")
        || report
            .reporter_tab_id
            .as_deref()
            .is_some_and(|reporter| reporter.trim() != terminal.executor_tab_id.trim())
        || !continuation_matches_review(task, terminal)
    {
        return None;
    }
    Some(index)
}

impl PlanStore {
    pub fn closed_review_continuation(
        &self,
        plan_id: &str,
        task_id: &str,
    ) -> Option<ReviewContinuation> {
        let plan = self.get(plan_id)?;
        let task_index = crate::plan::task_key::find_task_index(&plan.tasks, task_id)?;
        let task = &plan.tasks[task_index];
        let continuation_replay = task.status == TaskStatus::InProgress
            && matches!(
                task.status_label.as_deref(),
                Some("awaiting_continuation" | "bounce_continuation_failed" | "review bounced")
            );
        if (task.status != TaskStatus::InReview && !continuation_replay)
            || task.execution_attempts.iter().any(|attempt| {
                attempt.closed_at.is_none() && review_attempt_matches_task(task, attempt)
            })
        {
            return None;
        }
        let ClosedReviewAttempt::Unique(attempt_index) = exact_closed_review_attempt(task) else {
            return None;
        };
        let attempt = &task.execution_attempts[attempt_index];
        Some(ReviewContinuation {
            attempt_id: attempt.id.clone(),
            executor_tab_id: attempt.executor_tab_id.clone(),
            session_id: attempt.session_id.clone()?,
            mesh_peer: attempt.mesh_peer.clone().or_else(|| task.peer.clone()),
            epoch: attempt.continuation.epoch,
            review_round: task.review_round,
            state: attempt
                .continuation
                .state
                .clone()
                .unwrap_or_else(|| "executor_gone".into()),
        })
    }
}

impl PlanStore {
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
        self.bounce_review_with_recorded_status(
            plan_id,
            task_id,
            note,
            changed_executable_surfaces,
            distinct_oracle,
            from,
            now_ms,
        )
        .map(|(result, _)| result)
    }

    pub fn bounce_review_with_recorded_status(
        &self,
        plan_id: &str,
        task_id: &str,
        note: &str,
        changed_executable_surfaces: bool,
        distinct_oracle: bool,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<(ReviewTransitionResult, bool), PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mut result = None;
        let mut recorded = false;
        self.with_plan(plan_id, now_ms, |p| {
            let idx = crate::plan::task_key::find_task_index(&p.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            if matches!(
                p.tasks[idx].status,
                TaskStatus::InReview | TaskStatus::InProgress
            ) && matches!(
                p.tasks[idx].status_label.as_deref(),
                Some("awaiting_continuation" | "bounce_continuation_failed" | "review bounced")
            ) {
                if let Some(attempt_index) = exact_review_continuation_attempt(&p.tasks[idx]) {
                    let attempt = &p.tasks[idx].execution_attempts[attempt_index];
                    if attempt.continuation.review_round != Some(p.tasks[idx].review_round) {
                        return Err(PlanError::ReviewContinuationAttemptNotFound {
                            task_id: p.tasks[idx].id.clone(),
                        });
                    }
                    result = Some(ReviewTransitionResult {
                        plan_id: p.id.clone(),
                        task_id: p.tasks[idx].id.clone(),
                        milestone_id: attempt.continuation.milestone_id.clone().ok_or_else(
                            || PlanError::ReviewContinuationAttemptNotFound {
                                task_id: p.tasks[idx].id.clone(),
                            },
                        )?,
                        task_name: p.tasks[idx].name.clone(),
                        verdict: ReviewVerdict::Bounce,
                        review_round: p.tasks[idx].review_round,
                        follow_up_task_id: None,
                        strategy_warning: bounce_strategy_advice(&p.tasks[idx]),
                        continuation: Some(ReviewContinuation {
                            attempt_id: attempt.id.clone(),
                            executor_tab_id: attempt.executor_tab_id.clone(),
                            session_id: attempt.session_id.clone().ok_or_else(|| {
                                PlanError::ReviewContinuationAttemptNotFound {
                                    task_id: p.tasks[idx].id.clone(),
                                }
                            })?,
                            mesh_peer: attempt
                                .mesh_peer
                                .clone()
                                .or_else(|| p.tasks[idx].peer.clone()),
                            epoch: attempt.continuation.epoch,
                            review_round: p.tasks[idx].review_round,
                            state: attempt
                                .continuation
                                .state
                                .clone()
                                .unwrap_or_else(|| "awaiting_continuation".into()),
                        }),
                    });
                    return Ok(());
                }
                if p.tasks[idx].execution_attempts.iter().any(|attempt| {
                    attempt.closed_at.is_none()
                        && review_attempt_matches_task(&p.tasks[idx], attempt)
                }) {
                    return Err(PlanError::ReviewContinuationAttemptNotFound {
                        task_id: p.tasks[idx].id.clone(),
                    });
                }
                let task_id = p.tasks[idx].id.clone();
                let review_round = p.tasks[idx].review_round;
                let attempt_index = match exact_closed_review_attempt(&p.tasks[idx]) {
                    ClosedReviewAttempt::Unique(index) => index,
                    ClosedReviewAttempt::None | ClosedReviewAttempt::Ambiguous => {
                        return Err(PlanError::ReviewContinuationAttemptNotFound {
                            task_id: task_id.clone(),
                        })
                    }
                };
                let (milestone_id, task_name) = {
                    let task = &mut p.tasks[idx];
                    let attempt = &mut task.execution_attempts[attempt_index];
                    if attempt.continuation.review_round != Some(review_round) {
                        return Err(PlanError::ReviewContinuationAttemptNotFound { task_id });
                    }
                    let milestone_id =
                        attempt.continuation.milestone_id.clone().ok_or_else(|| {
                            PlanError::ReviewContinuationAttemptNotFound {
                                task_id: task_id.clone(),
                            }
                        })?;
                    attempt.continuation.state = Some("executor_gone".into());
                    attempt
                        .closure_reason
                        .get_or_insert_with(|| "review_bounce_executor_gone".into());
                    task.status = TaskStatus::Pending;
                    task.status_label = Some("review bounced".into());
                    task.status_changed_at = now_ms;
                    task.updated_at = now_ms;
                    (milestone_id, task.name.clone())
                };
                result = Some(ReviewTransitionResult {
                    plan_id: p.id.clone(),
                    task_id,
                    milestone_id,
                    task_name,
                    verdict: ReviewVerdict::Bounce,
                    review_round,
                    follow_up_task_id: None,
                    strategy_warning: bounce_strategy_advice(&p.tasks[idx]),
                    continuation: None,
                });
                return Ok(());
            }
            require_in_review(&p.tasks[idx])?;
            let open_attempts = p.tasks[idx]
                .execution_attempts
                .iter()
                .enumerate()
                .filter(|(_, attempt)| {
                    attempt.closed_at.is_none()
                        && review_attempt_matches_task(&p.tasks[idx], attempt)
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let (attempt_index, can_continue) = match open_attempts.as_slice() {
                [index] => (*index, true),
                [] => {
                    let attempt_index = match exact_closed_review_attempt(&p.tasks[idx]) {
                        ClosedReviewAttempt::Unique(index) => index,
                        ClosedReviewAttempt::None => cleaned_terminal_review_attempt(&p.tasks[idx])
                            .ok_or_else(|| PlanError::ReviewContinuationAttemptNotFound {
                                task_id: p.tasks[idx].id.clone(),
                            })?,
                        ClosedReviewAttempt::Ambiguous => {
                            return Err(PlanError::ReviewContinuationAttemptNotFound {
                                task_id: p.tasks[idx].id.clone(),
                            })
                        }
                    };
                    (attempt_index, false)
                }
                _ => {
                    return Err(PlanError::ReviewContinuationAttemptNotFound {
                        task_id: p.tasks[idx].id.clone(),
                    })
                }
            };
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
                task.review_round = task
                    .review_round
                    .checked_add(1)
                    .ok_or_else(|| PlanError::Io("review round exhausted".into()))?;
                let attempt = &mut task.execution_attempts[attempt_index];
                if can_continue {
                    attempt.continuation.epoch =
                        attempt.continuation.epoch.checked_add(1).ok_or_else(|| {
                            PlanError::Io("review continuation epoch exhausted".into())
                        })?;
                    attempt.continuation.review_round = Some(task.review_round);
                    attempt.continuation.state = Some("awaiting_continuation".into());
                    attempt.continuation.milestone_id = Some(mid.clone());
                    attempt.continuation.delivered_at = None;
                    task.status = TaskStatus::InReview;
                    task.status_label = Some("awaiting_continuation".into());
                } else {
                    attempt.continuation.review_round = Some(task.review_round);
                    attempt.continuation.state = Some("executor_gone".into());
                    attempt.continuation.milestone_id = Some(mid.clone());
                    attempt.continuation.delivered_at = None;
                    attempt
                        .closure_reason
                        .get_or_insert_with(|| "review_bounce_executor_gone".into());
                    task.status = TaskStatus::Pending;
                    task.status_label = Some("review bounced".into());
                }
                task.review_stage = ReviewStage::Review;
                task.qa_state = None;
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
                strategy_warning: bounce_strategy_advice(&p.tasks[idx]),
                continuation: can_continue.then(|| {
                    let attempt = &p.tasks[idx].execution_attempts[attempt_index];
                    ReviewContinuation {
                        attempt_id: attempt.id.clone(),
                        executor_tab_id: attempt.executor_tab_id.clone(),
                        session_id: attempt.session_id.clone().unwrap_or_default(),
                        mesh_peer: attempt
                            .mesh_peer
                            .clone()
                            .or_else(|| p.tasks[idx].peer.clone()),
                        epoch: attempt.continuation.epoch,
                        review_round: p.tasks[idx].review_round,
                        state: "awaiting_continuation".into(),
                    }
                }),
            });
            recorded = true;
            Ok(())
        })?;
        Ok((result.ok_or(PlanError::TaskNotFound)?, recorded))
    }
}
