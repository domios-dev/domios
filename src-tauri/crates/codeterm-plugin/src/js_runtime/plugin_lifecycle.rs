use crate::plugin_lifecycle::{PermissionChangedEvent, PluginLifecycle};

use super::vm::{call_json, PluginVm, VmError};

impl PluginLifecycle for PluginVm {
    fn id(&self) -> &str {
        self.plugin_id()
    }

    fn permission_allowed(&self, permission: &str) -> bool {
        self.lifecycle_permission_allowed(permission)
    }

    fn on_permission_changed(&self, event: &PermissionChangedEvent) -> Result<(), String> {
        let value = serde_json::to_value(event).map_err(|error| error.to_string())?;
        let result: serde_json::Value = match call_json(self, "onPermissionChanged", &[value]) {
            Ok(value) => value,
            Err(VmError::MissingFunction(_)) => return Ok(()),
            Err(error) => return Err(error.to_string()),
        };
        if let Some(error) = result.get("error").and_then(serde_json::Value::as_str) {
            return Err(error.to_string());
        }
        Ok(())
    }
}
