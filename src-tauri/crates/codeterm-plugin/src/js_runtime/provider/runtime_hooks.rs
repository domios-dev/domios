use super::JsProvider;
use crate::agent_runtime::*;
use serde_json::{json, Value};

impl JsProvider {
    pub(super) async fn js_runtime_plan(
        &self,
        ctx: &RuntimeContext,
        op: &RuntimeOperation,
    ) -> Result<RuntimePlan, RuntimeFailure> {
        let vm = self.vm.clone();
        let cap = self.runtime_capability()?;
        let ctx = ctx.clone();
        let op = op.clone();
        let deadline = std::time::Instant::now() + RUNTIME_HOOK_DEADLINE;
        tokio::task::spawn_blocking(move || {
            validate_runtime_operation(&ctx, &op)?;
            let plan = super::super::vm::call_runtime_hook(
                &vm,
                "runtimePlan",
                &[json!(ctx), json!(op)],
                deadline,
            )?;
            validate_runtime_plan(&cap, &ctx, &op, &plan)?;
            Ok(plan)
        })
        .await
        .map_err(join_failure)?
    }

    pub(super) async fn js_runtime_response(
        &self,
        ctx: &RuntimeContext,
        op: &RuntimeOperation,
        state: &Value,
        reply: &RpcReply,
    ) -> Result<RuntimeStep, RuntimeFailure> {
        let vm = self.vm.clone();
        let cap = self.runtime_capability()?;
        let ctx = ctx.clone();
        let op = op.clone();
        let state = state.clone();
        let mut reply = reply.clone();
        let deadline = std::time::Instant::now() + RUNTIME_HOOK_DEADLINE;
        tokio::task::spawn_blocking(move || {
            validate_runtime_operation(&ctx, &op)?;
            validate_runtime_state(&state)?;
            if matches!(op, RuntimeOperation::ReadHistory { .. }) {
                if let RpcReply::Success { value } = &mut reply {
                    sanitize_history_input(value);
                }
            }
            validate_runtime_reply(&reply)?;
            let step = super::super::vm::call_runtime_hook(
                &vm,
                "runtimeResponse",
                &[json!(ctx), json!(op), state, json!(reply)],
                deadline,
            )?;
            validate_runtime_step(&cap, &ctx, &op, &reply, &step)?;
            Ok(step)
        })
        .await
        .map_err(join_failure)?
    }

    pub(super) async fn js_runtime_reduce(
        &self,
        ctx: &RuntimeContext,
        state: &Value,
        frames: &[RuntimeFrame],
    ) -> Result<RuntimeReduction, RuntimeFailure> {
        let vm = self.vm.clone();
        let cap = self.runtime_capability()?;
        let ctx = ctx.clone();
        let state = state.clone();
        let frames = frames.to_vec();
        let deadline = std::time::Instant::now() + RUNTIME_HOOK_DEADLINE;
        tokio::task::spawn_blocking(move || {
            validate_runtime_context(&ctx)?;
            validate_runtime_state(&state)?;
            validate_runtime_frames(&cap, &frames)?;
            let reduction = super::super::vm::call_runtime_hook(
                &vm,
                "runtimeReduce",
                &[json!(ctx), state, json!(frames)],
                deadline,
            )?;
            validate_runtime_reduction(&ctx, &reduction)?;
            Ok(reduction)
        })
        .await
        .map_err(join_failure)?
    }

    fn runtime_capability(&self) -> Result<AgentRuntimeCapability, RuntimeFailure> {
        self.manifest
            .validate_agent_runtime()
            .map_err(|e| RuntimeFailure::new(RuntimeFailureCode::InvalidSchema, e.to_string()))?;
        self.manifest
            .capabilities
            .agent_runtime
            .clone()
            .ok_or_else(|| {
                RuntimeFailure::new(RuntimeFailureCode::Unsupported, "agentRuntime not declared")
            })
    }
}

fn join_failure(error: tokio::task::JoinError) -> RuntimeFailure {
    RuntimeFailure::new(
        RuntimeFailureCode::HookFailed,
        format!("runtime executor failed: {error}"),
    )
}

#[cfg(test)]
mod tests;
