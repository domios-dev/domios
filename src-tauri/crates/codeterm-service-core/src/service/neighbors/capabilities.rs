use super::{storage::Metadata, ServiceContext};
use crate::service::tab_credentials::TabCaller;
use codeterm_neighbors::permissions::Permission;

#[derive(Clone, Copy, Debug)]
pub enum Capability {
    MemoryRead,
    MemoryWrite,
    Secrets,
    Spawn,
    Files,
}

impl Capability {
    fn require(self, permission: &Permission) -> Result<(), String> {
        let reason = match self {
            Self::MemoryRead if permission.allowed && permission.allow_memory => return Ok(()),
            Self::MemoryRead => "neighbor_memory_not_allowed",
            Self::MemoryWrite => "neighbor_memory_read_only",
            Self::Secrets => "neighbor_secrets_not_allowed",
            Self::Spawn if permission.allowed && permission.allow_tab_requests => return Ok(()),
            Self::Spawn => "tab_requests_not_allowed",
            Self::Files if permission.allowed && permission.allow_files => return Ok(()),
            Self::Files => "neighbor_files_not_allowed",
        };
        Err(format!(
            "{reason}: This concierge capability is not allowed for the Neighbor"
        ))
    }
}

pub fn rpc_capability(method: &str) -> Option<Capability> {
    if method.starts_with("mem.secret.") || method.starts_with("secret.") {
        Some(Capability::Secrets)
    } else if matches!(method, "mem.search" | "mem.get" | "mem.wake_up") {
        Some(Capability::MemoryRead)
    } else if method.starts_with("mem.") {
        Some(Capability::MemoryWrite)
    } else if matches!(
        method,
        "tab.create"
            | "tab.new"
            | "agent.spawn"
            | "orchestration.new"
            | "plan.task.dispatch"
            | "plan.task.respawn"
            | "blueprint.apply"
            | "sentinel.apply"
            | "cron.create"
    ) {
        Some(Capability::Spawn)
    } else {
        None
    }
}

pub fn http_capability(method: &str, path: &str) -> Option<Capability> {
    if path == "/api/secrets" || path.starts_with("/api/secrets/") {
        Some(Capability::Secrets)
    } else if path.starts_with("/api/mem/") {
        let item = path
            .strip_prefix("/api/mem/items/")
            .is_some_and(|id| !id.is_empty() && !id.contains('/'));
        if method == "GET" && (item || matches!(path, "/api/mem/search" | "/api/mem/wake-up")) {
            Some(Capability::MemoryRead)
        } else {
            Some(Capability::MemoryWrite)
        }
    } else if method == "POST"
        && (matches!(
            path,
            "/api/tabs"
                | "/api/agents/spawn"
                | "/api/pty/spawn"
                | "/api/agents/helper"
                | "/api/mesh/worker-dispatch"
        ) || path.starts_with("/api/workspaces/") && path.ends_with("/tabs")
            || path.starts_with("/api/plans/")
                && (path.ends_with("/dispatch") || path.ends_with("/respawn"))
            || path.starts_with("/api/automation/blueprints/") && path.ends_with("/apply")
            || path == "/api/crons"
            || path.starts_with("/api/crons/") && path.ends_with("/trigger"))
    {
        Some(Capability::Spawn)
    } else {
        None
    }
}

impl ServiceContext {
    pub(super) fn remember_neighbor_concierge_accounts(
        &self,
        uid: &str,
        settings: &super::storage::Settings,
    ) {
        let known: Vec<_> = self
            .neighbor_cell()
            .concierge_accounts
            .lock()
            .keys()
            .cloned()
            .collect();
        let live: std::collections::HashSet<_> = {
            let mux = self.mux.read();
            known
                .into_iter()
                .filter(|id| mux.get_tab(id).is_some())
                .collect()
        };
        let mut accounts = self.neighbor_cell().concierge_accounts.lock();
        accounts.retain(|id, _| live.contains(id));
        for tab in settings.bindings.values() {
            accounts.insert(tab.clone(), uid.to_owned());
        }
    }

    fn verify_neighbor_capability_caller(&self, caller: &TabCaller) -> Result<(), String> {
        let mux = self.mux.read();
        if !mux.get_tab(&caller.tab_id).is_some_and(|tab| {
            tab.launch_session_authenticated()
                && tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
        }) {
            return Err("neighbor_forbidden: Concierge caller binding changed".into());
        }
        Ok(())
    }

    async fn neighbor_capability_permission_at(
        &self,
        metadata: &Metadata,
        caller: &TabCaller,
    ) -> Result<Option<(String, Permission)>, String> {
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        let Some((friendship, _)) = settings
            .bindings
            .iter()
            .find(|(_, tab)| **tab == caller.tab_id)
        else {
            return Ok(None);
        };
        self.verify_neighbor_capability_caller(caller)?;
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        Ok(Some((
            friendship.clone(),
            settings
                .permissions
                .get(friendship)
                .cloned()
                .unwrap_or_default(),
        )))
    }

    async fn neighbor_capability_permission(
        &self,
        caller: &TabCaller,
    ) -> Result<Option<(String, Permission)>, String> {
        {
            let mux = self.mux.read();
            let tab = mux.get_tab(&caller.tab_id).ok_or("neighbor_forbidden")?;
            if crate::service::general_agent::is_general_agent(tab)
                || tab.team_id.is_some()
                || tab.role.is_some()
            {
                return Ok(None);
            }
        }
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned();
        let session = match account.as_ref() {
            Some(account) => account.session().await,
            None => None,
        };
        let Some(session) = session else {
            return if self
                .neighbor_cell()
                .concierge_accounts
                .lock()
                .contains_key(&caller.tab_id)
            {
                Err("neighbor_forbidden: Concierge account is signed out".into())
            } else {
                Ok(None)
            };
        };
        let generation = self.neighbor_history_generation();
        if self
            .neighbor_cell()
            .concierge_accounts
            .lock()
            .get(&caller.tab_id)
            .is_some_and(|uid| uid != &session.identity.uid)
        {
            return Err("neighbor_forbidden: Concierge belongs to another account".into());
        }
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        self.remember_neighbor_concierge_accounts(&session.identity.uid, &settings);
        let result = self
            .neighbor_capability_permission_at(&metadata, caller)
            .await?;
        if result.is_none()
            && self
                .neighbor_cell()
                .concierge_accounts
                .lock()
                .contains_key(&caller.tab_id)
        {
            return Err("neighbor_forbidden: Concierge binding ended".into());
        }
        self.require_neighbor_generation(generation)?;
        let account = account.ok_or("neighbor_forbidden: Concierge account unavailable")?;
        if !super::same_session(account.session().await.as_ref(), &session) {
            return Err("neighbor_forbidden: Concierge account changed".into());
        }
        Ok(result)
    }

    pub async fn authorize_neighbor_capability(
        &self,
        caller: &TabCaller,
        capability: Capability,
    ) -> Result<(), String> {
        if let Some((_, permission)) = self.neighbor_capability_permission(caller).await? {
            capability.require(&permission)?;
        }
        Ok(())
    }

    pub async fn authorize_neighbor_file_read(
        &self,
        caller: &TabCaller,
        friendship_id: &str,
    ) -> Result<(), String> {
        let Some((friendship, permission)) = self.neighbor_capability_permission(caller).await?
        else {
            return Err("neighbor_files_not_allowed: A bound concierge is required".into());
        };
        if friendship != friendship_id {
            return Err("neighbor_files_not_allowed: Wrong friendship".into());
        }
        Capability::Files.require(&permission)
    }

    pub async fn authorize_neighbor_rpc(
        &self,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let Some(capability) = rpc_capability(method) else {
            return Ok(());
        };
        let Some(tab_id) = params.and_then(|params| params["caller_tab_id"].as_str()) else {
            return Ok(());
        };
        let binding_nonce = params
            .and_then(|params| params["session_binding_nonce"].as_str())
            .ok_or("neighbor_forbidden: Missing caller nonce")?;
        self.authorize_neighbor_capability(
            &TabCaller {
                tab_id: tab_id.into(),
                binding_nonce: binding_nonce.into(),
            },
            capability,
        )
        .await
    }
}

#[cfg(test)]
mod tests;
