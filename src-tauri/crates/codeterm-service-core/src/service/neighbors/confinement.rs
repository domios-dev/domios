use super::{
    storage::{Metadata, Settings},
    ServiceContext,
};
use codeterm_neighbors::{
    permissions::{MachineAccess, Permission},
    Result,
};
use codeterm_types::plan::ExecutionIntent;
use serde_json::{json, Value};

pub(super) async fn prepare_folder(
    metadata: &Metadata,
    friendship: &str,
    permission: &Permission,
) -> std::result::Result<String, String> {
    permission.validate().map_err(|error| error.to_string())?;
    let own = metadata
        .friend_folder(friendship)
        .map_err(|error| error.to_string())?;
    let selected = permission.folder.as_ref().map(std::path::PathBuf::from);
    tokio::task::spawn_blocking(move || {
        let path = selected.as_ref().unwrap_or(&own);
        if selected.is_none() {
            codeterm_util::private_file::create_dir_all(path).map_err(|error| error.to_string())?;
        }
        if !path.is_absolute() || !path.is_dir() {
            return Err("Neighbor folder must be an existing absolute directory".into());
        }
        let canonical = path.canonicalize().map_err(|error| error.to_string())?;
        canonical
            .to_str()
            .map(codeterm_paths::strip_verbatim_prefix)
            .ok_or("Neighbor folder must be Unicode".into())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn sandbox_label(
    capability: &codeterm_plugin::provider::ExecutionSandboxCapability,
    platform: &str,
    runtime: bool,
    access: MachineAccess,
) -> std::result::Result<Option<String>, String> {
    let (intent, label) = match access {
        MachineAccess::Full => return Ok(None),
        MachineAccess::None => {
            return Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string())
        }
        MachineAccess::ReadOnly => (ExecutionIntent::ReadOnly, "read-only"),
        MachineAccess::Folder => (ExecutionIntent::Mutating, "workspace-write"),
    };
    match capability.resolve(&intent, platform, runtime) {
        Ok(Some(_)) => Ok(Some(label.into())),
        _ => Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string()),
    }
}

impl ServiceContext {
    pub(super) fn neighbor_concierge_sandbox(
        &self,
        provider: &str,
        access: MachineAccess,
    ) -> std::result::Result<Option<String>, String> {
        let runtime = crate::service::agent_runtime::engine::requested(self, provider);
        let capability = self
            .agent_registry
            .with_provider(provider, |provider| provider.execution_sandbox_capability())
            .flatten()
            .unwrap_or_default();
        sandbox_label(
            &capability,
            crate::agent::provider::current_platform_tag(),
            runtime,
            access,
        )
    }

    pub(super) fn neighbor_permission_projection(
        &self,
        metadata: &Metadata,
        friendship: &str,
        permission: &Permission,
        settings: &Settings,
    ) -> Result<Value> {
        let own = metadata.friend_folder(friendship)?;
        let requested = permission
            .folder
            .clone()
            .unwrap_or_else(|| own.to_string_lossy().into_owned());
        let (cwd, sandbox) = {
            let mux = self.mux.read();
            settings
                .bindings
                .get(friendship)
                .and_then(|id| mux.get_tab(id))
                .map(|tab| {
                    (
                        tab.cwd.clone(),
                        tab.restorable_launch.execution_sandbox.clone(),
                    )
                })
                .unwrap_or_default()
        };
        let folder = cwd.unwrap_or(requested);
        let (writes, note) = match sandbox {
            Some(policy) => {
                let roots = if policy.writable_roots.is_empty() { String::new() }
                    else { format!(" Additional host writable roots: {}.", policy.writable_roots.join(", ")) };
                let writes = if policy.intent == ExecutionIntent::ReadOnly { "Filesystem writes are disabled by the provider sandbox." }
                    else { "Writes are limited to the folder by the provider sandbox." };
                ("provider_sandbox", format!("{writes}{roots} Reads are not confined on {}.", std::env::consts::OS))
            }
            None => (if permission.machine_access == MachineAccess::Full { "unrestricted" } else { "advisory" }, format!("This provider has no active OS filesystem sandbox on {}. The working folder is advisory; reads and writes are not confined.", std::env::consts::OS)),
        };
        let mut value = json!(permission);
        value["effective_folder"] = json!(folder);
        value["folder_enforcement"] = json!({"writes":writes,"reads":"not_confined","note":note});
        value["folder_enforcement"]["network"] = json!("not_confined");
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
