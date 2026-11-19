use super::ServiceContext;
use crate::{mesh, MeshStateHandle};
use mesh::machine_identity::MachineIdentity;
use serde_json::{json, Value};

pub(super) const MACHINE_IDENTITY_REQUEST: &str = "machine-identity:";

#[cfg(test)]
mod tests;

impl ServiceContext {
    pub fn update_machine_identity(
        &self,
        identity: MachineIdentity,
        expected: Option<u64>,
    ) -> Result<(), String> {
        let mesh = self
            .try_ext::<MeshStateHandle>()
            .ok_or("Mesh state is unavailable")?;
        if mesh::manager::set_machine_identity(mesh, identity, expected)? {
            self.ui.emit("mesh-identity-changed", Value::Null);
            let mesh = mesh.clone();
            crate::async_runtime::spawn(async move { mesh::broadcast_announce(&mesh, None).await });
        }
        Ok(())
    }

    pub fn request_machine_identity(&self, identity: MachineIdentity) -> Result<Value, String> {
        let mesh = self
            .try_ext::<MeshStateHandle>()
            .ok_or("Mesh state is unavailable")?;
        let (name, instance_id, revision) = {
            let manager = mesh.lock();
            (
                manager.ident.instance_name.clone(),
                manager.ident.identity.instance_id.clone(),
                manager.machine_identity_revision(),
            )
        };
        let payload =
            serde_json::to_string(&(instance_id, revision, identity, uuid::Uuid::new_v4()))
                .map_err(|error| error.to_string())?;
        let notification_id = self.publish_daemon_question(
            format!("{MACHINE_IDENTITY_REQUEST}{payload}"),
            format!("Change icon and colour for {name}?"),
            format!(
                "Apply the requested machine identity on {name}?\n\n{}",
                json!(identity)
            ),
            vec!["Apply".into(), "Cancel".into()],
        )?;
        Ok(json!({ "state": "awaiting_owner", "notification_id": notification_id }))
    }

    pub(super) fn answer_machine_identity(
        &self,
        request: &str,
        label: &str,
    ) -> Result<Value, String> {
        if label == "Cancel" {
            return Ok(json!({ "state": "cancelled" }));
        }
        if label != "Apply" {
            return Err("Invalid machine identity choice".into());
        }
        let payload = request
            .strip_prefix(MACHINE_IDENTITY_REQUEST)
            .ok_or("Invalid machine identity request")?;
        let (instance_id, revision, identity, _): (String, u64, MachineIdentity, uuid::Uuid) =
            serde_json::from_str(payload).map_err(|error| error.to_string())?;
        let mesh = self
            .try_ext::<MeshStateHandle>()
            .ok_or("Mesh state is unavailable")?;
        if mesh.lock().ident.identity.instance_id != instance_id {
            return Err("This request targets a different installation".into());
        }
        self.update_machine_identity(identity, Some(revision))?;
        Ok(json!({ "state": "saved", "identity": identity }))
    }
}
