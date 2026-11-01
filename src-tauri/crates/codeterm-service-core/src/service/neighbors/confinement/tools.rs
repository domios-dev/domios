use super::*;

impl ServiceContext {
    pub(super) fn neighbor_restore_access(
        &self,
        tab_id: &str,
    ) -> std::result::Result<MachineAccess, String> {
        let (root, bypass) = {
            let mux = self.mux.read();
            let tab = mux.get_tab(tab_id).ok_or("Neighbor concierge is missing")?;
            let authority = tab
                .neighbor_authority
                .as_ref()
                .ok_or("Neighbor authority is missing")?;
            let root = authority
                .root_tab_id
                .clone()
                .unwrap_or_else(|| tab_id.into());
            let bypass = self
                .neighbor_cell()
                .restore_grants
                .lock()
                .get(&root)
                .map(|grant| grant.access == MachineAccess::Full)
                .ok_or_else(|| codeterm_neighbors::Error::MachineAccessUnsupported.to_string())?;
            (root, bypass)
        };
        self.require_neighbor_restore_grant(tab_id, bypass)?;
        self.neighbor_cell()
            .restore_grants
            .lock()
            .get(&root)
            .map(|grant| grant.access)
            .ok_or_else(|| codeterm_neighbors::Error::MachineAccessUnsupported.to_string())
    }

    pub(crate) fn neighbor_scoped_tool_launch_args(
        &self,
        tab_id: &str,
        provider: &str,
        params: &crate::service::SpawnAgentParams,
    ) -> std::result::Result<Vec<String>, String> {
        let (authority, cwd) = {
            let mux = self.mux.read();
            let tab = mux
                .get_tab(tab_id)
                .ok_or("Neighbor launch tab is missing")?;
            (tab.neighbor_authority.clone(), tab.cwd.clone())
        };
        let Some(authority) = authority else {
            return Ok(Vec::new());
        };
        self.require_neighbor_concierge_receiver(&authority.friendship_id)
            .map_err(|error| error.to_string())?;
        let access = match params.neighbor_machine_access {
            Some(access) if params.neighbor_authority.as_ref() == Some(&authority) => access,
            Some(_) => return Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string()),
            None => self.neighbor_restore_access(tab_id)?,
        };
        if (access == MachineAccess::Full) != params.permission_bypass_requested(provider) {
            return Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string());
        }
        if access == MachineAccess::Full {
            return Ok(Vec::new());
        }
        if access == MachineAccess::None {
            return Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string());
        }
        if params.args.as_ref().is_some_and(|args| !args.is_empty()) {
            return Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string());
        }
        let capability = self
            .agent_registry
            .with_provider(provider, |provider| provider.launch_permission_capability())
            .flatten();
        let Some(capability) =
            capability.filter(|capability| capability.scoped_filesystem.is_some())
        else {
            if crate::service::execution_sandbox::resolve_requested(
                &self.agent_registry,
                provider,
                params,
                false,
            )?
            .is_some()
            {
                return Ok(Vec::new());
            }
            return Err(codeterm_neighbors::Error::MachineAccessUnsupported.to_string());
        };
        let pad = self.ensure_agent_pad(tab_id)?;
        let pad = pad
            .path()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let pad = pad
            .to_str()
            .map(codeterm_paths::strip_verbatim_prefix)
            .ok_or("Neighbor AgentPad must be Unicode")?;
        let mut writes = vec![pad];
        if access == MachineAccess::Folder {
            let folder = params
                .cwd
                .as_deref()
                .or(cwd.as_deref())
                .ok_or("Neighbor folder is missing")?;
            writes.insert(0, folder.into());
        }
        let reads = if access == MachineAccess::Folder {
            writes.clone()
        } else {
            Vec::new()
        };
        let mut args = capability.scoped_filesystem_args(
            access == MachineAccess::ReadOnly,
            &reads,
            &writes,
        )?;
        args.extend(capability.allow_command_args(&["domios"]));
        Ok(args)
    }
}

#[cfg(test)]
mod tests;
