use super::ServiceContext;

impl ServiceContext {
    pub async fn refresh_remote_task_environments(&self, provider_id: &str) -> Result<(), String> {
        if !self
            .chatbackend_registry
            .get(provider_id)
            .is_some_and(|provider| provider.remote_task_capability().is_some())
        {
            return Ok(());
        }
        let registry = self.chatbackend_registry.clone();
        let provider_id = provider_id.to_string();
        let completed =
            tokio::task::spawn_blocking(move || registry.refresh_and_wait(&provider_id))
                .await
                .map_err(|error| format!("Remote environment refresh failed: {error}"))?;
        if !completed {
            return Err("Remote environment refresh did not complete".into());
        }
        Ok(())
    }
}
