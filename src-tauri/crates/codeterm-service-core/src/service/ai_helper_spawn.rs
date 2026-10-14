use super::spawn::SpawnTabSpec;
use super::{ServiceContext, SpawnAgentParams, SpawnIdentityProvenance};
use crate::multiplexer::state::MultiplexerState;
use crate::service::general_agent::{general_dir, general_workspace_id};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiHelperRequest {
    pub title: String,
    pub description: String,
    pub task: String,
}

impl AiHelperRequest {
    pub(crate) fn new(title: String, description: String, task: String) -> Result<Self, String> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("AI helper spawn requires a title".into());
        }
        crate::service::tab::validate_tab_title(&title)?;
        let description = crate::service::normalize_tab_description(&description)?
            .ok_or("AI helper spawn requires a description")?;
        if task.trim().is_empty() {
            return Err("AI helper spawn requires a task".into());
        }
        Ok(Self {
            title,
            description,
            task,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiHelperPlacement {
    pub workspace_id: String,
    pub cwd: String,
}

pub(crate) struct AiHelperLaunch {
    pub tab_id: String,
    pub workspace_id: String,
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

pub(crate) fn ai_helper_provider(config: &crate::config::schema::AppConfig) -> String {
    config.general_agent.agent_provider.clone()
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
        description: Some(request.description.clone()),
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
        let provider = ai_helper_provider(&self.configuration());
        let params = ai_helper_params(&request, &placement, &provider);
        if self.classify_spawn_target_for_params(&provider, &params)? {
            let result = self.spawn_agent(params, None).await?;
            return Ok(AiHelperLaunch {
                tab_id: result.tab_id,
                workspace_id: placement.workspace_id,
                ready: None,
            });
        }
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
            ready: Some(ready_rx),
        })
    }
}

#[cfg(test)]
mod tests;
