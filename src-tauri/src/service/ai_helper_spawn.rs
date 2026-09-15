use super::spawn::SpawnTabSpec;
use super::{ServiceContext, SpawnAgentParams, SpawnIdentityProvenance};
use crate::multiplexer::state::MultiplexerState;
use crate::service::general_agent::{general_dir, general_workspace_id};
use crate::service::launch_delivery::{select_visible_spawn_task, visible_spawn_capabilities};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiHelperRequest {
    pub title: String,
    pub task: String,
}

impl AiHelperRequest {
    pub(crate) fn new(title: String, task: String) -> Result<Self, String> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("AI helper spawn requires a title".into());
        }
        if task.trim().is_empty() {
            return Err("AI helper spawn requires a task".into());
        }
        Ok(Self { title, task })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiHelperPlacement {
    pub workspace_id: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AiHelperDelivery {
    Confirmed,
    Launched,
}

impl AiHelperDelivery {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Launched => "launched",
        }
    }
}

pub(crate) struct AiHelperLaunch {
    pub tab_id: String,
    pub workspace_id: String,
    pub delivery: AiHelperDelivery,
    pub ready: Option<tokio::sync::oneshot::Receiver<Result<(), String>>>,
}

pub(crate) fn ai_helper_placement(mux: &MultiplexerState) -> Result<AiHelperPlacement, String> {
    let workspace_id = general_workspace_id(mux)
        .ok_or_else(|| "The General workspace is unavailable for the AI helper tab".to_string())?;
    Ok(AiHelperPlacement {
        workspace_id,
        cwd: general_dir().to_string_lossy().to_string(),
    })
}

/// Only a first turn submitted through the PTY carries submit evidence; an
/// inline starter reaches ready without proof that the turn began.
pub(crate) fn ai_helper_delivery(
    system_prompt_capable: bool,
    native_starter_capable: bool,
    task: &str,
) -> AiHelperDelivery {
    let selection = select_visible_spawn_task(
        None,
        Some(task),
        system_prompt_capable,
        native_starter_capable,
    );
    if selection.confirmed_first_turn.is_some() {
        AiHelperDelivery::Confirmed
    } else {
        AiHelperDelivery::Launched
    }
}

pub(crate) fn ai_helper_params(
    request: &AiHelperRequest,
    placement: &AiHelperPlacement,
    provider: &str,
) -> SpawnAgentParams {
    SpawnAgentParams {
        agent: Some(provider.to_string()),
        cwd: Some(placement.cwd.clone()),
        provisioned_cwd: Some(placement.cwd.clone()),
        title: Some(request.title.clone()),
        title_provenance: Some(SpawnIdentityProvenance::UserSupplied),
        task: Some(request.task.clone()),
        workspace_id: Some(placement.workspace_id.clone()),
        activate: Some(true),
        ..Default::default()
    }
}

impl ServiceContext {
    pub(crate) async fn spawn_ai_helper(
        &self,
        request: AiHelperRequest,
    ) -> Result<AiHelperLaunch, String> {
        let placement = ai_helper_placement(&self.mux.read())?;
        let provider = crate::config::default_agent_provider();
        let params = ai_helper_params(&request, &placement, &provider);
        if self.classify_spawn_target_for_params(&provider, &params)? {
            let result = self.spawn_agent(params, None).await?;
            return Ok(AiHelperLaunch {
                tab_id: result.tab_id,
                workspace_id: placement.workspace_id,
                delivery: AiHelperDelivery::Launched,
                ready: None,
            });
        }
        let (system_prompt_capable, native_starter_capable) = self
            .agent_registry
            .with_provider(&provider, visible_spawn_capabilities)
            .ok_or_else(|| format!("General Agent provider '{provider}' is not registered"))?;
        let delivery =
            ai_helper_delivery(system_prompt_capable, native_starter_capable, &request.task);
        // The visible spawn path starts a roleless task with a first turn; the
        // orchestrated path launches it with no starter and leaves the composer empty.
        let spec = SpawnTabSpec::from_agent_params(params);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let svc = self.clone();
        let handle =
            tokio::task::spawn_blocking(move || svc.spawn_tab_with_ready(spec, Some(ready_tx)))
                .await
                .map_err(|error| format!("AI helper spawn task failed: {error}"))??;
        Ok(AiHelperLaunch {
            tab_id: handle.tab_id,
            workspace_id: handle.workspace_id,
            delivery,
            ready: Some(ready_rx),
        })
    }
}

#[cfg(test)]
mod tests;
