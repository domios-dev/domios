use super::platform::{self, RecordingInputDevice};
use crate::service::ServiceContext;

impl ServiceContext {
    pub(crate) async fn local_recording_input_devices(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<RecordingInputDevice>, String> {
        let service = self.clone();
        let id = plugin_id.to_string();
        tokio::task::spawn_blocking(move || service.local_recording_inputs(&id))
            .await
            .map_err(|_| "local_input_inventory_failed".to_string())?
    }

    pub(crate) fn local_recording_inputs(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<RecordingInputDevice>, String> {
        let status = self.local_recording_status(plugin_id)?;
        if !status.granted {
            return Err("local_recording_grant_required".into());
        }
        let trigger = self
            .local_recording_registry
            .declaration(plugin_id)
            .ok_or("local_recording_not_registered")?;
        let inputs = platform::input_devices(&trigger)?;
        let current = self.local_recording_status(plugin_id)?;
        if !current.granted || current.generation != status.generation {
            return Err("local_recording_configuration_changed".into());
        }
        Ok(inputs)
    }
}
