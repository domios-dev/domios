use super::{ServiceContext, SpawnAgentParams};

fn next_fork_title(source: &str, existing: &[&str]) -> String {
    let base = format!("{source} Fork");
    let mut candidate = base.clone();
    let mut number = 2;
    while existing.contains(&candidate.as_str()) {
        candidate = format!("{base} ({number})");
        number += 1;
    }
    candidate
}

#[derive(Debug, serde::Serialize)]
pub struct ForkResult {
    pub tab_id: String,
    pub source_tab_id: String,
    pub source_session_id: String,
    pub session_id: Option<String>,
    pub binding_pending: bool,
}
impl ServiceContext {
    pub(crate) fn name_fork_tab(&self, tab_id: &str, source_title: &str) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            let titles: Vec<_> = mux.all_tabs().map(|tab| tab.title.as_str()).collect();
            let title = next_fork_title(source_title, &titles);
            // Host-generated fork names use the title model, outside the CLI's one-word contract.
            mux.rename_tab(tab_id, &title)?;
        }
        self.emit_state_changed(super::MuxEmitCause::Title);
        super::session_persist::persist_session_after_mutation(
            self,
            super::session_persist::SessionPersistUrgency::Prompt,
        )?;
        Ok(())
    }

    pub fn agent_fork(&self, source_tab: &str) -> Result<ForkResult, String> {
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
        self.name_fork_tab(&result.tab_id, &source.title)?;
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
    fn fork_titles_keep_source_and_number_collisions() {
        assert_eq!(super::next_fork_title("Scout", &[]), "Scout Fork");
        assert_eq!(
            super::next_fork_title("Scout", &["Scout Fork"]),
            "Scout Fork (2)"
        );
        assert_eq!(
            super::next_fork_title("Scout", &["Scout Fork", "Scout Fork (2)"]),
            "Scout Fork (3)"
        );
        assert_eq!(super::next_fork_title("Scout Fork", &[]), "Scout Fork Fork");
        assert_eq!(
            super::next_fork_title("Scout", &["Scout Fork", "Scout Fork (3)"]),
            "Scout Fork (2)"
        );
    }

    #[test]
    fn agent_fork_host_intent_cannot_be_injected_through_spawn_json() {
        let params: super::SpawnAgentParams =
            serde_json::from_value(serde_json::json!({"fork_session":true})).unwrap();
        assert!(!params.fork_session);
    }
}
