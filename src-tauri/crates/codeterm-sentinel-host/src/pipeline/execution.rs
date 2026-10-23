use super::*;

pub struct RunCtx<'a> {
    pub handle: &'a Handle,
    pub service: &'a ServiceContext,
    pub lease: &'a Lease,
    pub run_id: &'a str,
    pub deadline: Instant,
    pub outputs: std::collections::BTreeMap<String, ExecutionEnvelope>,
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
            outputs: Default::default(),
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

    pub fn resolve_binding(&self, path: &str) -> Result<ExecutionEnvelope, JournalReason> {
        let mut parts = path.split('.');
        let name = parts.next().ok_or(JournalReason::ContractViolation)?;
        if parts.next() != Some("out") {
            return Err(JournalReason::ContractViolation);
        }
        let original = self
            .outputs
            .get(name)
            .ok_or(JournalReason::ContractViolation)?;
        let fields = parts.collect::<Vec<_>>();
        if fields.is_empty() {
            return Ok(original.clone());
        }
        let mut value = &original.value;
        for field in fields {
            value = if let Some(object) = value.as_object() {
                object.get(field)
            } else if let Some(array) = value.as_array() {
                field
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| array.get(index))
            } else {
                None
            }
            .ok_or(JournalReason::ContractViolation)?;
        }
        Ok(ExecutionEnvelope {
            media: "json".into(),
            schema_id: None,
            value: value.clone(),
            text: String::new(),
        })
    }

    fn retain_output(
        &mut self,
        name: &str,
        output: &ExecutionEnvelope,
    ) -> Result<(), JournalReason> {
        output.encode()?;
        if self.outputs.len() >= 64 && !self.outputs.contains_key(name) {
            return Err(JournalReason::BudgetExhausted);
        }
        self.outputs.insert(name.into(), output.clone());
        Ok(())
    }

    pub(super) fn record_skipped(
        &mut self,
        name: &str,
        reason: String,
    ) -> Result<(), JournalReason> {
        let marker = self.allocate_marker()?;
        required_journal(write_journal(
            self.handle,
            &self.lease.job,
            BlueprintJournalWrite::Decision {
                run_id: self.run_id.into(),
                marker,
                step_name: name.into(),
                outcome: reason.clone().into(),
                at_ms: now(self.handle),
                result_summary: None,
                reason: Some(reason.into()),
            },
        ))
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

#[derive(Debug)]
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
    if effect_after_claim(
        || {
            claim_step(
                ctx.handle,
                ctx.lease,
                ctx.run_id,
                marker,
                &step.name,
                now(ctx.handle),
            )
        },
        || (),
    )
    .is_none()
    {
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
        let input = if let Some(binding) = &contract.input_binding {
            ctx.resolve_binding(binding)?
        } else {
            input.clone()
        };
        contract.validate(&input, false)?;
        if matches!(
            step.kind,
            StepKind::Command { .. } | StepKind::Github { .. }
        ) && contract
            .limits
            .as_ref()
            .is_some_and(|limits| limits.mem_mb.is_some())
        {
            return Err(JournalReason::BudgetExhausted);
        }
        let operation_limit = ctx.remaining().min(Duration::from_millis(
            contract
                .limits
                .as_ref()
                .and_then(|limits| limits.wall_ms)
                .unwrap_or(u64::MAX),
        ));
        if operation_limit.is_zero() {
            return Err(JournalReason::Deadline);
        }
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
            operation_limit,
        );
        if result.uncertain {
            return Err(JournalReason::UncertainEffect);
        }
        if ctx.lease.cancelled.load(Ordering::SeqCst) {
            let reason = match step.kind {
                StepKind::Agent { .. } => "agent_cancelled",
                StepKind::Convert { .. } => "conversion_cancelled",
                _ => "skip:lease_cancelled",
            };
            return Err(JournalReason::Other(reason.into()));
        }
        let mut output = if matches!(step.kind, StepKind::Filter { .. } | StepKind::Ask { .. }) {
            input.clone()
        } else {
            ExecutionEnvelope::decode(result.next_text.as_bytes()).unwrap_or_else(
                |_| match contract
                    .output
                    .as_ref()
                    .and_then(|port| port.media.as_deref())
                {
                    Some("text") => ExecutionEnvelope {
                        media: "text".into(),
                        schema_id: None,
                        value: serde_json::Value::String(result.next_text.clone()),
                        text: String::new(),
                    },
                    Some(media @ ("file" | "audio" | "video" | "image")) => ExecutionEnvelope {
                        media: media.into(),
                        schema_id: None,
                        value: serde_json::Value::String(result.next_text.clone()),
                        text: String::new(),
                    },
                    _ => ExecutionEnvelope::from_text(&result.next_text),
                },
            )
        };
        if output.schema_id.is_none() {
            output.schema_id = contract
                .output
                .as_ref()
                .and_then(|port| port.reference.clone());
        }
        let output = ExecutionEnvelope::decode(&output.encode()?)?;
        if result.state == JournalState::Done {
            contract.validate(&output, true)?;
        }
        ctx.retain_output(&step.name, &output)?;
        Ok((result, output))
    })();
    match checked {
        Ok((result, output)) => {
            let body_ref = if matches!(step.kind, StepKind::Agent { .. }) {
                Some(
                    crate::agent_stage::persist_result(
                        ctx.run_id,
                        marker.index() as usize,
                        &result.next_text,
                    )
                    .map_err(|_| JournalReason::Other("agent_result_write_failed".into()))?,
                )
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
            if matches!(step.kind, StepKind::Command { .. }) && output == input {
                required_journal(write_journal(
                    ctx.handle,
                    &ctx.lease.job,
                    BlueprintJournalWrite::Decision {
                        run_id: ctx.run_id.into(),
                        marker,
                        step_name: step.name.clone(),
                        outcome: JournalOutcome::Unchanged,
                        at_ms: now(ctx.handle),
                        result_summary: Some("step output was unchanged".into()),
                        reason: None,
                    },
                ))?;
            }
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
                return Err(result.reason.unwrap_or(JournalReason::ContractViolation));
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
                } else if reason.as_str().starts_with("skip:") {
                    JournalState::Skipped
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
            | JournalReason::Deadline
            | JournalReason::BudgetExhausted
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
