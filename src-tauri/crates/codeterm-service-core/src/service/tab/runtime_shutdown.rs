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
}
