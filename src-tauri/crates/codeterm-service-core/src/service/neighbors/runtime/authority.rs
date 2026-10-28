use super::*;

const DENIED: &str = "neighbor_forbidden: This caller cannot access this Neighbor operation";

enum Access<'a> {
    Unrestricted,
    Friendship(&'a str),
    Delivery(&'a str),
}

impl ServiceContext {
    fn neighbor_runtime_access<'a>(
        &self,
        command: &'a NeighborRuntimeCommand,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<Access<'a>, String> {
        let Some(caller) = caller else {
            return Ok(Access::Unrestricted);
        };
        let mux = self.mux.read();
        let tab = mux.get_tab(caller).ok_or(DENIED)?;
        if !tab.launch_session_authenticated()
            || tab.session_binding_nonce.as_deref() != nonce
            || nonce.is_none()
        {
            return Err(DENIED.into());
        }
        if matches!(
            command,
            NeighborRuntimeCommand::Enroll { .. }
                | NeighborRuntimeCommand::Rotate
                | NeighborRuntimeCommand::Revoke
        ) {
            return Err(DENIED.into());
        }
        if crate::service::general_agent::is_general_agent(tab) && tab.neighbor_authority.is_none()
        {
            return Ok(Access::Unrestricted);
        }
        let root = tab
            .neighbor_authority
            .as_ref()
            .and_then(|authority| authority.root_tab_id.clone())
            .unwrap_or_else(|| caller.to_owned());
        drop(mux);
        self.validate_neighbor_gateway(&root).map_err(|_| DENIED)?;
        match command {
            NeighborRuntimeCommand::History { friendship_id, .. }
            | NeighborRuntimeCommand::Send { friendship_id, .. }
            | NeighborRuntimeCommand::Retry { friendship_id } => {
                Ok(Access::Friendship(friendship_id))
            }
            NeighborRuntimeCommand::DeliveryStatus { message_id } => {
                Ok(Access::Delivery(message_id))
            }
            _ => Err(DENIED.into()),
        }
    }

    pub(super) async fn authorize_neighbor_metadata(
        &self,
        command: &NeighborRuntimeCommand,
        caller: Option<&str>,
        nonce: Option<&str>,
        metadata: &Metadata,
    ) -> Result<(), String> {
        let friendship = match self.neighbor_runtime_access(command, caller, nonce)? {
            Access::Unrestricted => return Ok(()),
            Access::Friendship(id) => id.to_owned(),
            Access::Delivery(id) => metadata
                .outgoing_friendship(id)
                .await
                .map_err(|_| DENIED)?
                .ok_or(DENIED)?,
        };
        let settings = metadata.settings().await.map_err(|_| DENIED)?;
        let root = {
            let mux = self.mux.read();
            let tab = mux.get_tab(caller.ok_or(DENIED)?).ok_or(DENIED)?;
            match &tab.neighbor_authority {
                Some(authority) if authority.friendship_id == friendship => authority
                    .root_tab_id
                    .as_deref()
                    .unwrap_or(&tab.id)
                    .to_owned(),
                Some(_) => return Err(DENIED.into()),
                None => tab.id.clone(),
            }
        };
        if !hex_id(&friendship, 64) || settings.bindings.get(&friendship) != Some(&root) {
            return Err(DENIED.into());
        }
        if self.neighbor_effective_pause(&settings, &friendship) {
            return Err("neighbor_paused: This friendship's concierge is paused".into());
        }
        self.neighbor_runtime_access(command, caller, nonce)?;
        Ok(())
    }

    pub(super) async fn authorize_neighbor_runtime(
        &self,
        command: &NeighborRuntimeCommand,
        caller: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<(), String> {
        if matches!(
            self.neighbor_runtime_access(command, caller, nonce)?,
            Access::Unrestricted
        ) {
            return Ok(());
        }
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
            .ok_or(DENIED)?;
        let session = account.session().await.ok_or(DENIED)?;
        if caller.is_some_and(|caller| {
            self.mux
                .read()
                .get_tab(caller)
                .and_then(|tab| tab.neighbor_authority.as_ref())
                .is_some_and(|authority| authority.uid != session.identity.uid)
        }) {
            return Err(DENIED.into());
        }
        let generation = self.neighbor_history_generation();
        let metadata = Metadata::new(
            self.account_data_root().map_err(|_| DENIED)?,
            &session.identity.uid,
        );
        self.authorize_neighbor_metadata(command, caller, nonce, &metadata)
            .await?;
        if !super::super::same_session(account.session().await.as_ref(), &session)
            || self.require_neighbor_generation(generation).is_err()
        {
            return Err(DENIED.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
