use crate::service::ServiceContext;

#[cfg(any(test, feature = "test-support"))]
pub struct RuntimeStopOverride(pub fn(&ServiceContext, &str) -> Result<(), String>);

impl ServiceContext {
    pub fn shutdown_tab_agent_runtime(&self, tab_id: &str) -> Result<(), String> {
        self.chat_registry.shutdown_tab(tab_id);
        #[cfg(any(test, feature = "test-support"))]
        if let Some(stop) = self.host_extension::<RuntimeStopOverride>() {
            return (stop.0)(self, tab_id);
        }
        crate::service::agent_runtime::engine::stop_blocking(self, tab_id)
    }

    pub async fn shutdown_tab_agent_runtime_async(&self, tab_id: &str) -> Result<(), String> {
        let service = self.clone();
        let tab = tab_id.to_string();
        let overridden = tokio::task::spawn_blocking(move || {
            service.chat_registry.shutdown_tab(&tab);
            #[cfg(any(test, feature = "test-support"))]
            if let Some(stop) = service.host_extension::<RuntimeStopOverride>() {
                (stop.0)(&service, &tab)?;
                return Ok::<_, String>(true);
            }
            crate::service::agent_runtime::login::close_cli(&tab);
            Ok::<_, String>(false)
        })
        .await
        .map_err(|error| error.to_string())??;
        if overridden || crate::service::agent_runtime::engine::for_tab(tab_id).is_none() {
            return Ok(());
        }
        crate::service::agent_runtime::engine::stop(self, tab_id).await
    }
}
