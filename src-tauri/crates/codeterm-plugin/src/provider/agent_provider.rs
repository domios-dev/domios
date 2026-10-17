use super::*;
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[macro_use]
mod runtime_defaults;
#[macro_use]
mod provider_identity;
#[macro_use]
mod launch_readiness;
#[macro_use]
mod session_usage;
#[macro_use]
mod session_content;
#[macro_use]
mod prompt_detection;
#[macro_use]
mod model_behavior;
#[macro_use]
mod integration;
#[macro_use]
mod catalogue;

/// Trait for agent backend providers (Claude, Codex, Aider, etc.)
#[async_trait]
pub trait AgentProvider: Send + Sync {
    agent_provider_runtime_defaults!();

    async fn runtime_plan(
        &self,
        _ctx: &crate::agent_runtime::RuntimeContext,
        _op: &crate::agent_runtime::RuntimeOperation,
    ) -> Result<crate::agent_runtime::RuntimePlan, crate::agent_runtime::RuntimeFailure> {
        Err(crate::agent_runtime::RuntimeFailure::new(
            crate::agent_runtime::RuntimeFailureCode::Unsupported,
            "runtimePlan is unsupported",
        ))
    }

    async fn runtime_response(
        &self,
        _ctx: &crate::agent_runtime::RuntimeContext,
        _op: &crate::agent_runtime::RuntimeOperation,
        _state: &serde_json::Value,
        _reply: &crate::agent_runtime::RpcReply,
    ) -> Result<crate::agent_runtime::RuntimeStep, crate::agent_runtime::RuntimeFailure> {
        Err(crate::agent_runtime::RuntimeFailure::new(
            crate::agent_runtime::RuntimeFailureCode::Unsupported,
            "runtimeResponse is unsupported",
        ))
    }

    async fn runtime_reduce(
        &self,
        _ctx: &crate::agent_runtime::RuntimeContext,
        _state: &serde_json::Value,
        _frames: &[crate::agent_runtime::RuntimeFrame],
    ) -> Result<crate::agent_runtime::RuntimeReduction, crate::agent_runtime::RuntimeFailure> {
        Err(crate::agent_runtime::RuntimeFailure::new(
            crate::agent_runtime::RuntimeFailureCode::Unsupported,
            "runtimeReduce is unsupported",
        ))
    }
    agent_provider_identity!();
    agent_provider_launch_readiness!();
    agent_provider_session_usage!();
    agent_provider_session_content!();
    agent_provider_prompt_detection!();
    agent_provider_model_behavior!();
    agent_provider_integration!();
    agent_provider_catalogue_head!();

    /// Query the models this provider currently offers, live.
    /// None means "can't probe; use the manifest seed."
    async fn discover_models(&self) -> Option<Vec<ModelInfo>> {
        None
    }
    agent_provider_catalogue_tail!();
}
