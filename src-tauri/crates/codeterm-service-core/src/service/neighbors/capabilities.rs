use super::{storage::Metadata, ServiceContext};
use crate::service::tab_credentials::TabCaller;
use codeterm_neighbors::permissions::Permission;

#[cfg(any(test, feature = "test-support"))]
mod fixture;
mod memory;
mod secrets;
mod self_reads;
mod spawn;
mod workspaces;

#[derive(Clone, Copy, Debug)]
pub enum Capability {
    MemoryRead,
    MemoryWrite,
    Spawn,
    Files,
    Neighbor,
    Report,
    Answer,
    Lifecycle,
    SelfRead,
    Discovery,
    TabList,
    TabSend,
    Route,
    Denied,
}

impl Capability {
    fn require(self, permission: &Permission) -> Result<(), String> {
        let reason = match self {
            Self::MemoryRead if permission.allowed && permission.allow_memory => return Ok(()),
            Self::MemoryRead => "neighbor_memory_not_allowed",
            Self::MemoryWrite
                if permission.allowed && !permission.memory_write_areas.is_empty() =>
            {
                return Ok(())
            }
            Self::MemoryWrite => "neighbor_memory_write_not_allowed",
            Self::Spawn
                if permission.allowed
                    && permission.allow_tab_requests
                    && permission
                        .workspaces
                        .as_ref()
                        .is_some_and(|ids| !ids.is_empty()) =>
            {
                return Ok(())
            }
            Self::Spawn if permission.allowed && permission.allow_tab_requests => {
                "neighbor_capability_denied"
            }
            Self::Spawn => "tab_requests_not_allowed",
            Self::Files if permission.allowed && permission.allow_files => return Ok(()),
            Self::Files => "neighbor_files_not_allowed",
            Self::TabList | Self::TabSend
                if permission.allowed
                    && permission
                        .workspaces
                        .as_ref()
                        .is_some_and(|ids| !ids.is_empty()) =>
            {
                return Ok(())
            }
            Self::TabList | Self::TabSend => "neighbor_capability_denied",
            Self::Neighbor
            | Self::Report
            | Self::Answer
            | Self::Lifecycle
            | Self::SelfRead
            | Self::Discovery
            | Self::Route => return Ok(()),
            Self::Denied => "neighbor_capability_denied",
        };
        Err(format!(
            "{reason}: This concierge capability is not allowed for the Neighbor. Politely refuse the current request through the Neighbor channel with --kind refusal and --in-reply-to set to its message_id."
        ))
    }
}

pub fn rpc_capability(method: &str) -> Option<Capability> {
    Some(match method {
        "mem.search" | "mem.get" | "mem.wake_up" => Capability::MemoryRead,
        "mem.neighbor_save" => Capability::MemoryWrite,
        "tab.create" | "tab.new" | "agent.spawn" => Capability::Spawn,
        "agent.report" => Capability::Report,
        "tab.agent_pad" | "tab.todo.list" => Capability::SelfRead,
        "tab.list" => Capability::TabList,
        "tab.send_text" => Capability::TabSend,
        "system.identify" | "system.capabilities" => Capability::Discovery,
        "api.request" => Capability::Route,
        _ => Capability::Denied,
    })
}

pub fn http_capability(method: &str, path: &str) -> Option<Capability> {
    if method == "POST" && path == "/api/cli/rpc" {
        Some(Capability::Route)
    } else if method == "POST" && path == "/api/account/neighbors/runtime" {
        Some(Capability::Neighbor)
    } else if method == "POST" && path == "/api/account/neighbors/files" {
        Some(Capability::Files)
    } else if method == "POST" && path == "/api/mem/neighbor-save" {
        Some(Capability::MemoryWrite)
    } else if method == "GET" && path == "/api/tabs" {
        Some(Capability::TabList)
    } else if method == "POST"
        && path
            .strip_prefix("/api/tabs/")
            .and_then(|rest| rest.strip_suffix("/send"))
            .is_some_and(|id| !id.is_empty() && !id.contains('/'))
    {
        Some(Capability::TabSend)
    } else if method == "POST" && path.starts_with("/api/tabs/") && path.ends_with("/report") {
        Some(Capability::Report)
    } else if method == "POST" && lifecycle_hook(path) {
        Some(Capability::Lifecycle)
    } else if method == "GET"
        && path.starts_with("/api/tabs/")
        && path.ends_with("/todos")
        && path.matches('/').count() == 4
    {
        Some(Capability::SelfRead)
    } else if method == "POST"
        && path.starts_with("/api/tabs/")
        && path.contains("/agent/runtime/questions/")
        && path.ends_with("/answer")
    {
        Some(Capability::Answer)
    } else if method == "GET"
        && (matches!(path, "/api/mem/search" | "/api/mem/wake-up")
            || path
                .strip_prefix("/api/mem/items/")
                .is_some_and(|id| !id.is_empty() && !id.contains('/')))
    {
        Some(Capability::MemoryRead)
    } else if method == "POST"
        && (matches!(path, "/api/tabs" | "/api/agents/spawn")
            || path.starts_with("/api/workspaces/") && path.ends_with("/tabs"))
    {
        Some(Capability::Spawn)
    } else {
        Some(Capability::Denied)
    }
}

fn lifecycle_hook(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/api/tabs/") else {
        return false;
    };
    let Some((tab, event)) = rest.split_once("/hook/") else {
        return false;
    };
    !tab.is_empty()
        && !tab.contains('/')
        && matches!(event, "session_start" | "stop" | "context" | "permission")
}

impl ServiceContext {
    pub(super) fn tag_neighbor_root(
        &self,
        uid: &str,
        friendship: &str,
        tab_id: &str,
    ) -> codeterm_neighbors::Result<()> {
        if !codeterm_neighbors::wire::hex_id(friendship, 64) {
            return Err(codeterm_neighbors::Error::InvalidRequest);
        }
        let changed = {
            let mut mux = self.mux.write();
            let tab = mux
                .get_tab_mut(tab_id)
                .ok_or(codeterm_neighbors::Error::Gateway)?;
            let authority = crate::multiplexer::types::NeighborAuthority {
                uid: uid.into(),
                friendship_id: friendship.into(),
                root_tab_id: None,
            };
            match tab.neighbor_authority.as_ref() {
                Some(existing) if existing != &authority => {
                    return Err(codeterm_neighbors::Error::PermissionDenied)
                }
                Some(_) => false,
                None => {
                    tab.neighbor_authority = Some(authority);
                    true
                }
            }
        };
        if changed {
            self.emit_state_changed(crate::service::MuxEmitCause::TabMetadata);
        }
        Ok(())
    }

    pub(super) fn remember_neighbor_concierge_accounts(
        &self,
        uid: &str,
        settings: &super::storage::Settings,
    ) {
        self.clear_neighbor_restore_grants(uid);
        for (friendship, tab) in &settings.bindings {
            let _ = self.tag_neighbor_root(uid, friendship, tab);
            self.remember_neighbor_restore_grant(
                tab,
                settings
                    .permissions
                    .get(friendship)
                    .map(|permission| permission.machine_access)
                    .unwrap_or_default(),
            );
        }
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

    fn verify_neighbor_capability_caller(
        &self,
        caller: &TabCaller,
        require_attested: bool,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        if !mux.get_tab(&caller.tab_id).is_some_and(|tab| {
            (!require_attested || tab.launch_session_authenticated())
                && tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
        }) {
            return Err("neighbor_forbidden: Concierge caller binding changed".into());
        }
        Ok(())
    }

    #[cfg(test)]
    async fn neighbor_capability_permission_at(
        &self,
        metadata: &Metadata,
        caller: &TabCaller,
    ) -> Result<Option<(String, Permission)>, String> {
        let settings = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?;
        self.neighbor_capability_permission_in(&settings, caller, true)
    }

    fn neighbor_capability_permission_in(
        &self,
        settings: &super::storage::Settings,
        caller: &TabCaller,
        require_attested: bool,
    ) -> Result<Option<(String, Permission)>, String> {
        let Some((friendship, _)) = settings
            .bindings
            .iter()
            .find(|(_, tab)| **tab == caller.tab_id)
        else {
            return Ok(None);
        };
        self.verify_neighbor_capability_caller(caller, require_attested)?;
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
        self.neighbor_capability_permission_for(caller, true).await
    }

    async fn neighbor_capability_permission_for(
        &self,
        caller: &TabCaller,
        require_attested: bool,
    ) -> Result<Option<(String, Permission)>, String> {
        let authority = {
            let mux = self.mux.read();
            let tab = mux.get_tab(&caller.tab_id).ok_or("neighbor_forbidden")?;
            if crate::service::general_agent::is_general_agent(tab)
                && tab.neighbor_authority.is_none()
                && !self
                    .neighbor_cell()
                    .concierge_accounts
                    .lock()
                    .contains_key(&caller.tab_id)
            {
                return Ok(None);
            }
            tab.neighbor_authority.clone()
        };
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
            return if authority.is_some()
                || self
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
        if authority
            .as_ref()
            .is_some_and(|authority| authority.uid != session.identity.uid)
            || self
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
        let result = if let Some(authority) = authority {
            self.verify_neighbor_capability_caller(caller, require_attested)?;
            self.require_neighbors_enabled()
                .map_err(|error| error.to_string())?;
            let root = authority.root_tab_id.as_deref().unwrap_or(&caller.tab_id);
            if authority.root_tab_id.is_some()
                && !self.mux.read().get_tab(root).is_some_and(|tab| {
                    tab.neighbor_authority
                        .as_ref()
                        .is_some_and(|root_authority| {
                            root_authority.uid == authority.uid
                                && root_authority.friendship_id == authority.friendship_id
                                && root_authority.root_tab_id.is_none()
                        })
                })
            {
                return Err("neighbor_forbidden: Concierge root is no longer active".into());
            }
            if !codeterm_neighbors::wire::hex_id(&authority.friendship_id, 64)
                || require_attested
                    && settings
                        .bindings
                        .get(&authority.friendship_id)
                        .map(String::as_str)
                        != Some(root)
            {
                return Err("neighbor_forbidden: Concierge binding is not active".into());
            }
            let permission = settings
                .permissions
                .get(&authority.friendship_id)
                .cloned()
                .unwrap_or_default();
            Some((authority.friendship_id, permission))
        } else {
            self.neighbor_capability_permission_in(&settings, caller, require_attested)?
        };
        if let Some((friendship, _)) = &result {
            self.require_neighbor_concierge_receiver(friendship)
                .map_err(|error| error.to_string())?;
        }
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
        if require_attested
            && result
                .as_ref()
                .is_some_and(|(friendship, _)| self.neighbor_effective_pause(&settings, friendship))
        {
            return Err("neighbor_paused: This friendship's concierge is paused".into());
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

    pub async fn authorize_neighbor_http(
        &self,
        caller: &TabCaller,
        method: &str,
        path: &str,
    ) -> Result<(), String> {
        let path = path.split('?').next().unwrap_or(path);
        if secrets::secret_path(path) {
            self.authorize_neighbor_secret_caller(caller)?;
        }
        let capability =
            http_capability(&method.to_ascii_uppercase(), path).unwrap_or(Capability::Denied);
        let Some((friendship, permission)) = self
            .neighbor_capability_permission_for(
                caller,
                !matches!(capability, Capability::Lifecycle | Capability::SelfRead),
            )
            .await?
        else {
            return Ok(());
        };
        capability.require(&permission)?;
        if matches!(capability, Capability::TabSend) {
            let target = path
                .strip_prefix("/api/tabs/")
                .and_then(|rest| rest.strip_suffix("/send"))
                .ok_or("neighbor_capability_denied")?;
            self.require_neighbor_workspace_target(&permission, &friendship, target)?;
        }
        if matches!(capability, Capability::MemoryRead) {
            let item = path.strip_prefix("/api/mem/items/");
            self.authorize_neighbor_memory_operation(&permission, path == "/api/mem/wake-up", item)
                .await?;
        }
        if matches!(
            capability,
            Capability::Answer | Capability::Report | Capability::Lifecycle | Capability::SelfRead
        ) {
            let target = path
                .strip_prefix("/api/tabs/")
                .and_then(|path| path.split('/').next());
            if target
                .and_then(|id| self.mux.read().resolve_tab_id(id))
                .as_deref()
                != Some(caller.tab_id.as_str())
            {
                return Capability::Denied.require(&permission);
            }
        }
        Ok(())
    }

    pub async fn authorize_neighbor_rpc_credential(
        &self,
        credential: &str,
        method: &str,
        params: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let Some(caller) = self.authenticate_local_credential(credential)? else {
            return Ok(());
        };
        self.authorize_neighbor_secret_rpc(&caller, method, params)?;
        let own_launch_hook = method == "api.request"
            && params.is_some_and(|params| {
                params["method"].as_str() == Some("POST")
                    && params["path"].as_str().is_some_and(lifecycle_hook)
            });
        let intrinsic = matches!(
            rpc_capability(method),
            Some(Capability::SelfRead | Capability::Discovery)
        );
        let Some((friendship, permission)) = self
            .neighbor_capability_permission_for(&caller, !own_launch_hook && !intrinsic)
            .await?
        else {
            return Ok(());
        };
        if params
            .and_then(|params| params["caller_tab_id"].as_str())
            .is_some_and(|id| id != caller.tab_id)
        {
            return Capability::Denied.require(&permission);
        }
        let capability = rpc_capability(method).unwrap_or(Capability::Denied);
        capability.require(&permission)?;
        if matches!(capability, Capability::Spawn) {
            self.require_neighbor_spawn_workspace(
                &permission,
                params.and_then(|params| params["workspace_id"].as_str()),
            )?;
        }
        if matches!(capability, Capability::TabSend) {
            let target = params
                .and_then(|params| params["tab_id"].as_str())
                .ok_or("neighbor_capability_denied: Missing target")?;
            self.require_neighbor_workspace_target(&permission, &friendship, target)?;
        }
        if matches!(capability, Capability::SelfRead)
            && params.and_then(|params| params["tab_id"].as_str()) != Some(caller.tab_id.as_str())
        {
            return Capability::Denied.require(&permission);
        }
        if matches!(capability, Capability::MemoryRead) {
            self.authorize_neighbor_memory_operation(
                &permission,
                method == "mem.wake_up",
                if method == "mem.get" {
                    params.and_then(|params| params["id"].as_str())
                } else {
                    None
                },
            )
            .await?;
        }
        if matches!(capability, Capability::Report) {
            caller.validate_claim(params.and_then(|params| params["tab_id"].as_str()))?;
        }
        if matches!(capability, Capability::Route) {
            let method = params
                .and_then(|params| params["method"].as_str())
                .ok_or("neighbor_capability_denied: Missing route method")?;
            let path = params
                .and_then(|params| params["path"].as_str())
                .ok_or("neighbor_capability_denied: Missing route path")?;
            self.authorize_neighbor_http(&caller, method, path).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod tests;
