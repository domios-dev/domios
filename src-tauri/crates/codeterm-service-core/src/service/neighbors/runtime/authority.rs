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
        if crate::service::general_agent::is_general_agent(tab) {
            return Ok(Access::Unrestricted);
        }
        drop(mux);
        self.validate_neighbor_gateway(caller).map_err(|_| DENIED)?;
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
        if !hex_id(&friendship, 64)
            || settings.bindings.get(&friendship).map(String::as_str) != caller
        {
            return Err(DENIED.into());
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
