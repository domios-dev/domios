use super::*;
use crate::converter::ConversionCancellation;
use std::{sync::Arc, time::Instant};

pub struct StepRuntime<'a> {
    pub ledger: &'a OperationLedger,
    pub descriptor: &'a StepDescriptor,
    pub adapter: Arc<dyn BlueprintStep>,
    pub media: Option<&'a dyn StepMedia>,
}

impl StepRuntime<'_> {
    pub fn drive(
        &self,
        plugin: &str,
        request: &StepRequest,
        now_ms: impl FnOnce() -> u64,
        deadline: Instant,
        cancellation: ConversionCancellation,
    ) -> Result<OperationRecord, String> {
        self.validate(plugin, request)?;
        let Some(_lease) = self.ledger.try_acquire(&request.context.operation) else {
            return self.record(request);
        };
        let existing = self.ledger.prepare(plugin, request)?;
        if existing.state.terminal()
            || matches!(
                existing.state,
                OperationState::Queued | OperationState::Running | OperationState::Unknown
            )
        {
            return Ok(existing);
        }
        if existing.state == OperationState::Submitting {
            self.ledger.change(&request.context.operation, |record| {
                if record.state == OperationState::Submitting {
                    record.state = OperationState::Unknown;
                }
                Ok(())
            })?;
            return self.record(request);
        }
        if existing.state == OperationState::WaitingForOwnerOk && existing.authorized.is_none() {
            return Ok(existing);
        }
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            return self.cancel_before_submit(request);
        }
        let call = StepCall {
            request,
            job: None,
            deadline,
            cancellation: cancellation.clone(),
        };
        let estimate = if self.descriptor.effects.potentially_paid {
            self.adapter.estimate(&call).ok().flatten()
        } else {
            Some(Estimate {
                amount: 0,
                unit: self.unit(request)?,
                valid_until_ms: u64::MAX,
            })
        };
        let admitted = self
            .ledger
            .reserve(&request.context.operation, estimate, now_ms())?;
        if admitted.state != OperationState::Submitting {
            return Ok(admitted);
        }
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            return self.cancel_before_submit(request);
        }
        let current = self.record(request)?;
        if current.cancel_requested {
            return Ok(current);
        }
        let update = self.adapter.submit(&call);
        self.settle(request, update)?;
        self.record(request)
    }

    pub fn reconcile(
        &self,
        request: &StepRequest,
        deadline: Instant,
        cancellation: ConversionCancellation,
    ) -> Result<OperationRecord, String> {
        let _lease = self
            .ledger
            .try_acquire(&request.context.operation)
            .ok_or("plugin operation busy")?;
        let record = self.record(request)?;
        self.validate(&record.plugin, request)?;
        if record.state.terminal()
            || matches!(
                record.state,
                OperationState::Prepared | OperationState::WaitingForOwnerOk
            )
        {
            return Ok(record);
        }
        if !self.descriptor.lifecycle.reconcile {
            return Err("step reconciliation unavailable".into());
        }
        if cancellation.is_cancelled() || Instant::now() >= deadline {
            return Err("step reconciliation interrupted".into());
        }
        let call = StepCall {
            request,
            job: record.job.as_deref(),
            deadline,
            cancellation,
        };
        self.settle(request, self.adapter.reconcile(&call))?;
        self.record(request)
    }

    pub fn cancel(
        &self,
        request: &StepRequest,
        deadline: Instant,
    ) -> Result<OperationRecord, String> {
        let Some(_lease) = self.ledger.try_acquire(&request.context.operation) else {
            return self.record_cancel_intent(request);
        };
        let record = self.record(request)?;
        self.validate(&record.plugin, request)?;
        if record.state.terminal() {
            return Ok(record);
        }
        if matches!(
            record.state,
            OperationState::Prepared | OperationState::WaitingForOwnerOk
        ) {
            return self.cancel_before_submit(request);
        }
        if record.job.is_none() {
            return self.record_cancel_intent(request);
        }
        self.record_cancel_intent(request)?;
        if !self.descriptor.lifecycle.cancel || Instant::now() >= deadline {
            return self.record(request);
        }
        let call = StepCall {
            request,
            job: record.job.as_deref(),
            deadline,
            cancellation: Default::default(),
        };
        self.settle(request, self.adapter.cancel(&call))?;
        self.record(request)
    }

    fn validate(&self, plugin: &str, request: &StepRequest) -> Result<(), String> {
        self.descriptor.validate()?;
        if !token(&request.unit) || request.step_allowance > MAX_COST_AMOUNT {
            return Err("invalid step spending authorization".into());
        }
        if !self.descriptor.id.starts_with(&format!("{plugin}."))
            || request.node_type != self.descriptor.id
            || request.revision != self.descriptor.revision
        {
            return Err("step capability or revision unavailable".into());
        }
        let context = &request.context;
        for value in [
            &context.scope.workspace,
            &context.scope.run,
            &context.owner,
            &context.node,
            &context.operation,
            &context.attempt,
        ] {
            if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
                return Err("invalid trusted step identity".into());
            }
        }
        if context.run_label.len() > 256
            || context.node_label.len() > 128
            || context.run_view.len() > 512
            || context.input_lineage.len() > 16
            || context
                .run_label
                .chars()
                .chain(context.node_label.chars())
                .any(char::is_control)
            || serde_json::to_vec(request)
                .map_err(|_| "invalid step request")?
                .len()
                > 2 * MAX_STEP_BYTES
        {
            return Err("step context exceeds bounds".into());
        }
        self.descriptor
            .params
            .validate_instance(&request.params)
            .map_err(|_| "plugin step parameters violate schema")?;
        self.descriptor.validate_values(&request.inputs, false)?;
        if self
            .descriptor
            .outputs
            .values()
            .any(|p| !matches!(p.kind, PortKind::Text | PortKind::Json))
            && self.media.is_none()
        {
            return Err("run media authority unavailable".into());
        }
        let mut lineage = Vec::new();
        for value in request.inputs.values().flatten() {
            if let PortValue::Media { handle, media } = value {
                let source = self
                    .media
                    .ok_or("run media authority unavailable")?
                    .input(context, plugin, handle, *media)?;
                if !lineage.contains(&source) {
                    lineage.push(source);
                }
            }
        }
        if lineage != context.input_lineage {
            return Err("run media attribution mismatch".into());
        }
        Ok(())
    }

    fn unit(&self, request: &StepRequest) -> Result<String, String> {
        self.ledger.run_unit(&request.context.scope, &request.unit)
    }

    fn record(&self, request: &StepRequest) -> Result<OperationRecord, String> {
        let record = self
            .ledger
            .get(&request.context.operation)?
            .ok_or("operation missing")?;
        let mut expected = request.clone();
        expected.context.attempt = record.request.context.attempt.clone();
        if expected != record.request {
            return Err("operation scope or identity changed".into());
        }
        Ok(record)
    }

    fn cancel_before_submit(&self, request: &StepRequest) -> Result<OperationRecord, String> {
        self.ledger.change(&request.context.operation, |record| {
            if matches!(
                record.state,
                OperationState::Prepared | OperationState::WaitingForOwnerOk
            ) {
                record.state = OperationState::Canceled;
                record.waiting = None;
            } else if !record.state.terminal() {
                record.cancel_requested = true;
                record.state = OperationState::Unknown;
            }
            Ok(())
        })?;
        self.record(request)
    }

    fn record_cancel_intent(&self, request: &StepRequest) -> Result<OperationRecord, String> {
        self.record(request)?;
        self.ledger.change(&request.context.operation, |record| {
            if !record.state.terminal() {
                record.cancel_requested = true;
                record.state = OperationState::Unknown;
            }
            Ok(())
        })?;
        self.record(request)
    }

    fn settle(
        &self,
        request: &StepRequest,
        update: Result<JobUpdate, String>,
    ) -> Result<(), String> {
        let update = update.ok();
        let mut identity_accepted = false;
        self.ledger.change(&request.context.operation, |record| {
            if let Some(job) = update
                .as_ref()
                .map(|update| &update.job)
                .filter(|job| valid_job_identity(job))
            {
                match &record.job {
                    None => {
                        record.job = Some(job.clone());
                        record.state = OperationState::Unknown;
                        identity_accepted = true;
                    }
                    Some(existing) if existing == job => identity_accepted = true,
                    _ => (),
                }
            }
            if !record.state.terminal() {
                record.state = OperationState::Unknown;
            }
            Ok(())
        })?;
        if !identity_accepted {
            return Ok(());
        }
        let existing = self.record(request)?;
        if existing.state.terminal() {
            return Ok(());
        }
        let update = update
            .filter(|u| {
                existing.job.as_ref().is_none_or(|job| job == &u.job)
                    && matches!(
                        u.state,
                        OperationState::Queued
                            | OperationState::Running
                            | OperationState::Succeeded
                            | OperationState::Failed
                            | OperationState::Canceled
                            | OperationState::Unknown
                    )
                    && u.outcome
                        .as_ref()
                        .is_none_or(|o| self.descriptor.outcomes.contains(o))
                    && (u.state != OperationState::Succeeded || u.outcome.is_some())
                    && (u.outputs.is_empty() && u.state != OperationState::Succeeded
                        || self.descriptor.validate_values(&u.outputs, true).is_ok())
            })
            .and_then(|mut update| {
                for (port, values) in &mut update.outputs {
                    for value in values {
                        match value {
                            PortValue::MediaSource { id, media } => {
                                let handle = self
                                    .media?
                                    .publish(&request.context, &existing.plugin, port, id, *media)
                                    .ok()?;
                                *value = PortValue::Media {
                                    handle,
                                    media: *media,
                                };
                            }
                            PortValue::Media { .. } => return None,
                            _ => (),
                        }
                    }
                }
                Some(update)
            });
        self.ledger.change(&request.context.operation, |record| {
            if record.state.terminal() {
                return Ok(());
            }
            match update {
                Some(update) if record.job.as_ref().is_none_or(|job| job == &update.job) => {
                    record.job = Some(update.job);
                    record.state = if record.cancel_requested && !update.state.terminal() {
                        OperationState::Unknown
                    } else {
                        update.state
                    };
                    record.outcome = update.outcome;
                    if !update.outputs.is_empty() {
                        record.outputs = update.outputs;
                    }
                }
                _ => record.state = OperationState::Unknown,
            }
            Ok(())
        })
    }
}
