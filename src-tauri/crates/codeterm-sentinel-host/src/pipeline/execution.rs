use super::*;

pub struct RunCtx<'a> {
    pub handle: &'a Handle,
    pub service: &'a ServiceContext,
    pub lease: &'a Lease,
    pub run_id: &'a str,
    pub deadline: Instant,
    next_marker: u16,
    call_depth: u8,
    pub(crate) input_kind: Option<MediaKind>,
    pub(crate) input_source: Option<std::path::PathBuf>,
    pub(crate) input_failure: Option<String>,
    pub(crate) input_display: String,
    pub(crate) cleanup: Option<std::sync::Arc<crate::input::Cleanup>>,
}

impl<'a> RunCtx<'a> {
    pub fn new(
        handle: &'a Handle,
        service: &'a ServiceContext,
        lease: &'a Lease,
        run_id: &'a str,
    ) -> Self {
        let remaining = lease
            .job
            .expires_at
            .map(|at| at.saturating_sub(now(handle)))
            .unwrap_or(300_000);
        Self {
            handle,
            service,
            lease,
            run_id,
            deadline: Instant::now() + Duration::from_millis(remaining),
            next_marker: 0,
            call_depth: 0,
            input_kind: None,
            input_source: None,
            input_failure: None,
            input_display: String::new(),
            cleanup: None,
        }
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn enter_call(&mut self) -> Result<(), JournalReason> {
        if self.call_depth == 8 {
            return Err(JournalReason::BudgetExhausted);
        }
        self.call_depth += 1;
        Ok(())
    }

    pub fn exit_call(&mut self) {
        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn allocate_marker(&mut self) -> Result<JournalMarker, JournalReason> {
        if self.next_marker >= 64 {
            return Err(JournalReason::BudgetExhausted);
        }
        let marker = JournalMarker::Step(self.next_marker);
        self.next_marker += 1;
        Ok(marker)
    }
}

pub struct StepExecution {
    pub output: ExecutionEnvelope,
    pub outcome: StepOutcome,
    pub stored_outcome: JournalOutcome,
    pub state: JournalState,
    pub summary: String,
    pub reason: Option<JournalReason>,
    pub uncertain: bool,
}

pub fn execute_step(
    ctx: &mut RunCtx<'_>,
    step: &Step,
    input: ExecutionEnvelope,
) -> Result<StepExecution, JournalReason> {
    let marker = ctx.allocate_marker()?;
    if !claim_step(
        ctx.handle,
        ctx.lease,
        ctx.run_id,
        marker,
        &step.name,
        now(ctx.handle),
    ) {
        return Err(JournalReason::UncertainClaim);
    }
    let contract = step.execution.clone().unwrap_or_default();
    let checked = (|| {
        if ctx.lease.cancelled.load(Ordering::SeqCst) {
            return Err(JournalReason::Other("skip:lease_cancelled".into()));
        }
        if ctx.remaining().is_zero() {
            return Err(JournalReason::Deadline);
        }
        contract.validate(&input, false)?;
        let result = dispatch::run_step(
            ctx.handle,
            ctx.service,
            ctx.lease,
            step,
            &input,
            ctx.input_kind,
            ctx.input_source.as_deref(),
            ctx.input_failure.as_deref(),
            &ctx.input_display,
            ctx.cleanup.clone(),
        );
        if result.uncertain {
            return Err(JournalReason::UncertainEffect);
        }
        let output = if matches!(step.kind, StepKind::Filter { .. } | StepKind::Ask { .. }) {
            input.clone()
        } else {
            ExecutionEnvelope::decode(result.next_text.as_bytes())
                .unwrap_or_else(|_| ExecutionEnvelope::from_text(&result.next_text))
        };
        if result.state == JournalState::Done {
            contract.validate(&output, true)?;
        }
        Ok((result, output))
    })();
    match checked {
        Ok((result, output)) => {
            let body_ref = if matches!(step.kind, StepKind::Agent { .. }) {
                crate::agent_stage::persist_result(
                    ctx.run_id,
                    marker.index() as usize,
                    &result.next_text,
                )
                .ok()
            } else {
                None
            };
            finish(
                ctx,
                marker,
                result.state,
                result.stored_outcome.clone(),
                result.summary.clone(),
                body_ref,
                result.reason.clone(),
            )?;
            if let Some((outcome, reason)) = result.decision {
                required_journal(write_journal(
                    ctx.handle,
                    &ctx.lease.job,
                    BlueprintJournalWrite::Decision {
                        run_id: ctx.run_id.into(),
                        marker,
                        step_name: step.name.clone(),
                        outcome,
                        at_ms: now(ctx.handle),
                        result_summary: None,
                        reason,
                    },
                ))?;
            }
            if result.reason.as_ref().is_some_and(fatal) {
                return Err(result.reason.unwrap_or(JournalReason::SandboxDenied));
            }
            Ok(StepExecution {
                output,
                outcome: result.outcome,
                stored_outcome: result.stored_outcome,
                state: result.state,
                summary: result.summary,
                reason: result.reason,
                uncertain: result.uncertain,
            })
        }
        Err(reason) => {
            let uncertain = matches!(reason, JournalReason::UncertainEffect);
            finish(
                ctx,
                marker,
                if uncertain {
                    JournalState::Held
                } else {
                    JournalState::Failed
                },
                if uncertain { "held" } else { "failed" }.into(),
                format!("node stopped: {}", reason.as_str()),
                None,
                Some(reason.clone()),
            )?;
            Err(reason)
        }
    }
}

fn fatal(reason: &JournalReason) -> bool {
    matches!(
        reason,
        JournalReason::ContractViolation
            | JournalReason::SandboxDenied
            | JournalReason::Deadline
            | JournalReason::BudgetExhausted
            | JournalReason::UnsupportedPlatform
            | JournalReason::RuntimeUnavailable
    )
}

fn finish(
    ctx: &RunCtx<'_>,
    marker: JournalMarker,
    state: JournalState,
    outcome: JournalOutcome,
    summary: String,
    body_ref: Option<String>,
    reason: Option<JournalReason>,
) -> Result<(), JournalReason> {
    required_journal(write_journal(
        ctx.handle,
        &ctx.lease.job,
        BlueprintJournalWrite::Finish {
            run_id: ctx.run_id.into(),
            marker,
            state,
            outcome,
            ended_at_ms: now(ctx.handle),
            result_summary: StepJournalEntry::bound_summary(&summary),
            body_ref,
            reason,
        },
    ))
}
