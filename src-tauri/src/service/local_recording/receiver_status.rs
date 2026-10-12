use crate::service::ServiceContext;
use codeterm_plugin::local_recording::ReceiverStatusQuery;

impl ServiceContext {
    pub(crate) fn read_receiver_status_report(
        &self,
        plugin_id: &str,
        query: &ReceiverStatusQuery,
    ) -> Result<Vec<u8>, String> {
        if !query.valid() {
            return Err("receiver_status_invalid_declaration".into());
        }
        let status = self.local_recording_status(plugin_id)?;
        if !status.granted || !status.enabled || !status.available {
            return Err("receiver_status_unavailable".into());
        }
        let (devices, generation) = self
            .local_recording_registry
            .selected_devices(plugin_id)
            .ok_or("devices_unavailable")?;
        #[cfg(windows)]
        let report = super::platform::windows::receiver_status::read(query, &devices.input_id)?;
        #[cfg(not(windows))]
        let report = {
            let _ = devices;
            Vec::<u8>::new()
        };
        let current = self.local_recording_status(plugin_id)?;
        if !current.granted
            || !current.enabled
            || !current.available
            || current.generation != generation
        {
            return Err("local_recording_configuration_changed".into());
        }
        Ok(report)
    }
}
