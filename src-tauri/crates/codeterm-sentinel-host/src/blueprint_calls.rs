use crate::pipeline::{execute_step, RunCtx};
use codeterm_blueprint_types::blueprint_document::calls::{
    self, CallFrame, CallPin, FrozenClosure,
};
use codeterm_blueprint_types::blueprint_document::{Call, Document, Node, ValueForm};
use codeterm_blueprint_types::blueprint_execution::ExecutionEnvelope;
use codeterm_blueprint_types::blueprint_journal::{JournalMarker, JournalReason, JournalState};
use codeterm_blueprint_types::blueprint_pipeline::{StepKind, StepOutcome};
use codeterm_storage::{BlueprintJournalWrite, NestedClaimParent};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub struct CallContext {
    pub frames: Vec<CallFrame>,
    pub inputs: BTreeMap<String, Value>,
    pub(crate) simulate: bool,
    pub effects: Vec<SimulatedEffect>,
    pub grants: BTreeSet<String>,
    pub current_marker: Option<JournalMarker>,
    parents: Vec<JournalMarker>,
    pub stop: Option<CallStop>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct SimulatedEffect {
    pub scope: &'static str,
    pub node: String,
    pub call_stack: Vec<CallFrame>,
    pub effect: Value,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct CallStop {
    pub node: String,
    pub call_stack: Vec<CallFrame>,
    pub reason: JournalReason,
}
impl CallContext {
    pub fn parent(&self) -> Option<NestedClaimParent> {
        self.parents
            .last()
            .zip(self.frames.last())
            .map(|(marker, frame)| NestedClaimParent {
                marker: *marker,
                frame: frame.clone(),
            })
    }
    pub fn stack_for(&self, name: &str, kind: &StepKind) -> Vec<CallFrame> {
        let mut frames = self.frames.clone();
        if let StepKind::Call { call, .. } = kind {
            if let Some(pin) = &call.pin {
                frames.push(CallFrame {
                    node: name.into(),
                    id: pin.id.clone(),
                    hash: pin.hash.clone(),
                });
            }
        }
        frames
    }
    pub fn enter(
        &mut self,
        node: &str,
        pin: &CallPin,
        marker: JournalMarker,
    ) -> Result<(), JournalReason> {
        if self.frames.len() >= calls::MAX_CALL_DEPTH {
            return Err(JournalReason::BudgetExhausted);
        }
        if self.frames.iter().any(|frame| frame.id == pin.id) {
            return Err(JournalReason::ContractViolation);
        }
        self.frames.push(CallFrame {
            node: node.into(),
            id: pin.id.clone(),
            hash: pin.hash.clone(),
        });
        self.parents.push(marker);
        Ok(())
    }
    pub fn exit(&mut self) {
        self.frames.pop();
        self.parents.pop();
    }
}
pub(crate) fn claim_step(ctx: &RunCtx<'_>, marker: JournalMarker, name: &str, at: u64) -> bool {
    crate::pipeline::write_journal(
        ctx.handle,
        &ctx.lease.job,
        BlueprintJournalWrite::Claim {
            definition_version: Some(ctx.lease.job.definition_version),
            parent: ctx.call_state.parent(),
            run_id: ctx.run_id.into(),
            marker,
            step_name: name.into(),
            generation: ctx.lease.job.revision,
            started_at_ms: at,
        },
    )
}
pub(crate) fn metadata(
    ctx: &RunCtx<'_>,
    marker: JournalMarker,
    name: &str,
    kind: &StepKind,
) -> Result<(), JournalReason> {
    let call_stack = ctx.call_state.stack_for(name, kind);
    if call_stack.is_empty() {
        return Ok(());
    }
    if call_stack.len() > calls::MAX_CALL_DEPTH {
        return Err(JournalReason::BudgetExhausted);
    }
    if crate::pipeline::write_journal(
        ctx.handle,
        &ctx.lease.job,
        BlueprintJournalWrite::CallMetadata {
            run_id: ctx.run_id.into(),
            marker,
            call_stack,
        },
    ) {
        Ok(())
    } else {
        Err(JournalReason::JournalWriteFailed)
    }
}
pub(crate) fn execute(
    ctx: &mut RunCtx<'_>,
    name: &str,
    call: &Call,
    grants: &[String],
    _input: &ExecutionEnvelope,
) -> Result<crate::pipeline::StepResult, JournalReason> {
    let pin = call.pin.as_ref().ok_or(JournalReason::ContractViolation)?;
    if calls::saved_reference(&call.reference) && call.reference != pin.id {
        return Err(JournalReason::ContractViolation);
    }
    let document: Document = serde_json::from_value(
        ctx.lease
            .job
            .document
            .clone()
            .ok_or(JournalReason::ContractViolation)?,
    )
    .map_err(|_| JournalReason::ContractViolation)?;
    let mut root = document.clone();
    let dependencies = std::mem::take(&mut root.dependencies);
    let closure = FrozenClosure {
        root: codeterm_blueprint_types::blueprint_document::Parsed {
            document: root,
            locations: BTreeMap::new(),
        },
        callees: dependencies,
    };
    for callee in closure.callees.values() {
        callee
            .verify()
            .map_err(|_| JournalReason::ContractViolation)?;
    }
    let team = ctx
        .lease
        .job
        .team_id
        .as_deref()
        .ok_or(JournalReason::ContractViolation)?;
    if closure
        .callees
        .values()
        .any(|c| calls::saved_reference(&c.id))
    {
        ctx.service
            .mem
            .blueprint_call_drift(
                &format!("blueprints:{}", ctx.lease.job.workspace_id),
                team,
                document,
            )
            .map_err(|error| {
                if error.contains("budget_exhausted") {
                    JournalReason::BudgetExhausted
                } else {
                    JournalReason::ContractViolation
                }
            })?;
    }
    let callee = closure
        .callee(pin)
        .map_err(|_| JournalReason::ContractViolation)?;
    let outputs = ctx
        .outputs
        .iter()
        .map(|(id, value)| (id.clone(), value.value.clone()))
        .collect();
    let mut values = calls::arguments(call, &ctx.call_state.inputs, &outputs)
        .map_err(|_| JournalReason::ContractViolation)?;
    for (name, port) in &callee.document.inputs {
        if !values.contains_key(name) {
            if let Some(value) = &port.default {
                values.insert(name.clone(), value.clone());
            }
        }
    }
    let assignments = values
        .iter()
        .map(|(key, value)| {
            format!(
                "{key}={}",
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_owned)
            )
        })
        .collect::<Vec<_>>();
    let bound = callee
        .document
        .bind(&assignments)
        .map_err(|_| JournalReason::ContractViolation)?;
    let bound: Document =
        serde_json::from_value(bound).map_err(|_| JournalReason::ContractViolation)?;
    let allowed = grants.iter().cloned().collect::<BTreeSet<_>>();
    if !ctx.call_state.frames.is_empty() && !allowed.is_subset(&ctx.call_state.grants) {
        return Err(JournalReason::ContractViolation);
    }
    preflight(ctx, &closure, &bound, &allowed, &mut BTreeSet::new())?;
    ctx.enter_call()?;
    let marker = ctx
        .call_state
        .current_marker
        .ok_or(JournalReason::ContractViolation)?;
    if let Err(error) = ctx.call_state.enter(name, pin, marker) {
        ctx.exit_call();
        return Err(error);
    }
    let saved_inputs = std::mem::replace(&mut ctx.call_state.inputs, values.clone());
    let saved_outputs = std::mem::take(&mut ctx.outputs);
    let saved_grants = std::mem::replace(&mut ctx.call_state.grants, allowed);
    let result = run_callee(ctx, &bound, &values);
    ctx.outputs = saved_outputs;
    ctx.call_state.inputs = saved_inputs;
    ctx.call_state.grants = saved_grants;
    ctx.call_state.exit();
    ctx.exit_call();
    result.map(|output| success(&output, "call completed"))
}
fn run_callee(
    ctx: &mut RunCtx<'_>,
    document: &Document,
    values: &BTreeMap<String, Value>,
) -> Result<ExecutionEnvelope, JournalReason> {
    let initial = if values.len() == 1 {
        values.values().next().cloned().unwrap_or(Value::Null)
    } else {
        json!(values)
    };
    let mut input = envelope(initial)?;
    let nodes = document.nodes().collect::<Vec<_>>();
    let mut index = 0;
    while let Some(node) = nodes.get(index) {
        let step =
            calls::lower::node(document, node).map_err(|_| JournalReason::RuntimeUnavailable)?;
        let executed = execute_step(ctx, &step, input.clone());
        let result = match executed {
            Ok(result) if result.state == JournalState::Done => result,
            Ok(result) => {
                let reason = result
                    .reason
                    .unwrap_or_else(|| JournalReason::Other("callee_failed".into()));
                record_stop(ctx, node, &reason);
                return Err(reason);
            }
            Err(reason) => {
                record_stop(ctx, node, &reason);
                return Err(reason);
            }
        };
        input = result.output;
        let route = codeterm_blueprint_types::blueprint_pipeline::route_target(
            &step.on,
            match &result.outcome {
                StepOutcome::Done => "done",
                StepOutcome::Failed => "failed",
                StepOutcome::Bucket(bucket) => bucket,
            },
        );
        if route == "stop" {
            break;
        }
        if route == "next" {
            index += 1;
        } else {
            let next = nodes
                .iter()
                .position(|n| n.id == route)
                .ok_or(JournalReason::ContractViolation)?;
            if next <= index {
                return Err(JournalReason::ContractViolation);
            }
            index = next;
        }
    }
    let from = document
        .result
        .from
        .as_deref()
        .ok_or(JournalReason::ContractViolation)?;
    let output = ctx.resolve_binding(from)?;
    if !document
        .result
        .port
        .as_ref()
        .is_some_and(|port| port.accepts_with_types(&output.value, &document.types))
    {
        return Err(JournalReason::ContractViolation);
    }
    Ok(output)
}
fn record_stop(ctx: &mut RunCtx<'_>, node: &Node, reason: &JournalReason) {
    if ctx.call_state.stop.is_none() {
        ctx.call_state.stop = Some(CallStop {
            node: node.id.clone(),
            call_stack: ctx.call_state.frames.clone(),
            reason: reason.clone(),
        });
    }
}
fn preflight(
    ctx: &RunCtx<'_>,
    closure: &FrozenClosure,
    document: &Document,
    allowed: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
) -> Result<(), JournalReason> {
    if !document.other.is_empty() {
        return Err(JournalReason::RuntimeUnavailable);
    }
    for node in document.nodes() {
        if ctx.call_state.simulate
            && node.call.is_none()
            && node.kind() != "filter"
            && !node
                .uses
                .as_ref()
                .is_some_and(codeterm_blueprint_types::blueprint_document::NodeKind::effect)
        {
            return Err(JournalReason::RuntimeUnavailable);
        }
        if node
            .caps
            .iter()
            .chain(node.code.iter().flat_map(|code| &code.caps))
            .any(|cap| cap == "domios-cli" || !allowed.contains(cap))
        {
            return Err(JournalReason::ContractViolation);
        }
        if node
            .uses
            .as_ref()
            .is_some_and(codeterm_blueprint_types::blueprint_document::NodeKind::effect)
        {
            route(ctx, node)?;
        }
        if let Some(call) = &node.call {
            let pin = call.pin.as_ref().ok_or(JournalReason::ContractViolation)?;
            if !seen.insert(pin.id.clone()) {
                return Err(JournalReason::ContractViolation);
            }
            let child = closure
                .callee(pin)
                .map_err(|_| JournalReason::ContractViolation)?;
            preflight(ctx, closure, &child.document, allowed, seen)?;
            seen.remove(&pin.id);
        }
    }
    Ok(())
}
pub(crate) fn effect(
    ctx: &mut RunCtx<'_>,
    node: &Node,
    input: &ExecutionEnvelope,
) -> Result<crate::pipeline::StepResult, JournalReason> {
    let route = route(ctx, node)?;
    let payload = match node
        .params
        .get("payload")
        .or_else(|| node.params.get("text"))
    {
        Some(ValueForm::Bind { bind }) => ctx.resolve_binding(bind)?,
        Some(ValueForm::Lit { lit }) => envelope(lit.clone())?,
        Some(ValueForm::Input { input }) => envelope(
            ctx.call_state
                .inputs
                .get(input)
                .cloned()
                .ok_or(JournalReason::ContractViolation)?,
        )?,
        None => input.clone(),
    };
    if ctx.call_state.simulate {
        if ctx.call_state.effects.len() >= 64 {
            return Err(JournalReason::BudgetExhausted);
        }
        ctx.call_state.effects.push(SimulatedEffect {
            scope: "simulation",
            node: node.id.clone(),
            call_stack: ctx.call_state.frames.clone(),
            effect: calls::projection::bounded(
                &json!({"kind":node.kind(),"payload":payload.value}),
                0,
            ),
        });
    } else {
        crate::routes::run(
            ctx.service,
            &ctx.lease.job,
            &crate::machine::Verdict {
                bucket: "done".into(),
                confidence: 1.0,
            },
            &route,
            ctx.run_id,
            &payload.payload_text(),
        )
        .map_err(|_| JournalReason::ContractViolation)?;
    }
    Ok(success(
        input,
        if ctx.call_state.simulate {
            "effect simulated"
        } else {
            "effect accepted"
        },
    ))
}
fn route(ctx: &RunCtx<'_>, node: &Node) -> Result<crate::machine::Route, JournalReason> {
    use crate::machine::{EscalateTarget, Route};
    let target = || {
        node.params
            .get("target")
            .and_then(ValueForm::value)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(JournalReason::ContractViolation)
    };
    let route = match node.kind() {
        "notify" => Route::Notify,
        "send" => Route::Send(target()?),
        "cron" => Route::Cron(target()?),
        "blueprint" | "stop" | "respond_prompt" => return Err(JournalReason::RuntimeUnavailable),
        "escalate" => Route::Escalate(EscalateTarget::Creator),
        _ => return Err(JournalReason::RuntimeUnavailable),
    };
    crate::routes::resolve(ctx.service, &ctx.lease.job, &route)
        .map_err(|_| JournalReason::ContractViolation)
}
fn envelope(value: Value) -> Result<ExecutionEnvelope, JournalReason> {
    let envelope = ExecutionEnvelope {
        media: "json".into(),
        schema_id: None,
        value,
        text: String::new(),
    };
    envelope.encode()?;
    Ok(envelope)
}
fn success(output: &ExecutionEnvelope, label: &str) -> crate::pipeline::StepResult {
    crate::pipeline::StepResult {
        outcome: StepOutcome::Done,
        stored_outcome: "done".into(),
        state: JournalState::Done,
        summary: format!("{label}: {}", calls::projection::bounded(&output.value, 0)),
        reason: None,
        next_text: String::from_utf8(output.encode().unwrap_or_default()).unwrap_or_default(),
        decision: None,
        uncertain: false,
    }
}
#[cfg(test)]
mod tests;

pub fn project(
    document: &Document,
    entries: &[codeterm_storage::BlueprintJournalEntry],
) -> Result<Value, String> {
    project_retained(Some(document), entries, None)
}

pub fn project_retained(
    document: Option<&Document>,
    entries: &[codeterm_storage::BlueprintJournalEntry],
    definition_version: Option<u64>,
) -> Result<Value, String> {
    let latest = entries.first().map(|entry| entry.run_id.as_str());
    let mut seen = BTreeSet::new();
    let nodes = entries
        .iter()
        .filter(|entry| Some(entry.run_id.as_str()) == latest)
        .filter(|entry| seen.insert(entry.marker()))
        .take(66)
        .map(|entry| calls::projection::bounded(&json!(entry), 0))
        .collect::<Vec<_>>();
    let mut value = json!({"expanded_calls":[],"call_graph":null,"historical_graph_available":document.is_some(),"graph_definition_version":definition_version,"nested_run":{"id":latest,"definition_version":definition_version,"nodes":nodes}});
    if let Some(document) = document {
        let closure = FrozenClosure::from_document(document.clone())?;
        let graph = calls::projection::graph(&closure)?;
        value["expanded_calls"] = json!(graph.calls);
        value["call_graph"] = graph.root;
    }
    Ok(value)
}

pub fn pin_bundle(
    service: &codeterm_service_core::service::ServiceContext,
    caller: &crate::service_adapter::SentinelCaller,
    document: Document,
    local_callees: &[calls::LocalCallee],
) -> Result<Document, String> {
    service.mem.pin_blueprint_bundle(
        &format!("blueprints:{}", caller.workspace),
        caller.team.as_deref().ok_or("Call requires a Team")?,
        document,
        local_callees,
    )
}
pub fn show_callable(
    service: &codeterm_service_core::service::ServiceContext,
    caller: &crate::service_adapter::SentinelCaller,
    id: &str,
) -> Result<Value, String> {
    let document = service.mem.blueprint_callable(
        &format!("blueprints:{}", caller.workspace),
        caller
            .team
            .as_deref()
            .ok_or("Saved callable requires a Team")?,
        id,
    )?;
    let mut body = document.clone();
    body.dependencies.clear();
    let hash = codeterm_blueprint_types::blueprint_document::projection::identity(&body)?;
    let mut value = project(&document, &[])?;
    value["id"] = json!(id);
    value["hash"] = json!(hash);
    value["name"] = json!(document.name);
    value["saved_solution"] = json!(true);
    Ok(value)
}
