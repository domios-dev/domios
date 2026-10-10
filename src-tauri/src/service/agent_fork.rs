use super::{ServiceContext, SpawnAgentParams};

#[derive(Debug, serde::Serialize)]
pub(crate) struct ForkResult {
    pub tab_id: String,
    pub source_tab_id: String,
    pub source_session_id: String,
    pub session_id: Option<String>,
    pub binding_pending: bool,
}
impl ServiceContext {
    pub(crate) fn agent_fork(&self, source_tab: &str) -> Result<ForkResult, String> {
        let (source, workspace) = {
            let mux = self.mux.read();
            let source = mux
                .get_tab(source_tab)
                .cloned()
                .ok_or("source tab not found")?;
            let workspace = mux
                .snapshot()
                .workspaces
                .into_iter()
                .find(|workspace| workspace.tabs.contains(&source.id))
                .map(|workspace| workspace.id);
            (source, workspace)
        };
        let provider = source
            .agent_type
            .as_deref()
            .ok_or("source tab has no provider")?;
        let session = source
            .agent_session_id
            .as_deref()
            .ok_or("source tab has no verified session")?;
        let cwd = source
            .cwd
            .as_deref()
            .ok_or("source session working directory is unavailable")?;
        let supported = self
            .agent_registry
            .with_provider(provider, |provider| provider.fork_session_args().is_some())
            .unwrap_or(false);
        if !supported {
            return Err(
                "session_fork_unsupported: this provider does not declare session forks".into(),
            );
        }
        if !self
            .agent_registry
            .with_provider(provider, |provider| {
                provider.session_exists(cwd, session) == Some(true)
            })
            .unwrap_or(false)
        {
            return Err("source session transcript is unavailable".into());
        }
        let result = self.spawn_agent_sync(SpawnAgentParams {
            agent: Some(provider.into()),
            workspace_id: workspace,
            cwd: Some(cwd.into()),
            title: Some("Fork".into()),
            description: Some(format!("Fork of tab {}", source.id)),
            resume_session_id: Some(session.into()),
            fork_session: true,
            model: source
                .restorable_launch
                .model
                .clone()
                .or(source.agent_provider_resolved_model.clone()),
            reasoning_effort: source.restorable_launch.resolved_reasoning_effort.clone(),
            activate: Some(true),
            ..Default::default()
        })?;
        let bound = self
            .mux
            .read()
            .get_tab(&result.tab_id)
            .and_then(|tab| tab.agent_session_id.clone());
        Ok(ForkResult {
            tab_id: result.tab_id,
            source_tab_id: source.id.clone(),
            source_session_id: session.into(),
            binding_pending: bound.is_none(),
            session_id: bound,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_fork_host_intent_cannot_be_injected_through_spawn_json() {
        let params: super::SpawnAgentParams =
            serde_json::from_value(serde_json::json!({"fork_session":true})).unwrap();
        assert!(!params.fork_session);
    }
}
