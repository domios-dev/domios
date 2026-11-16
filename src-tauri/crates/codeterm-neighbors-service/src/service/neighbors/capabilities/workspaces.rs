use super::*;
use crate::prelude::*;
use serde_json::Value;

pub trait NeighborsExt {
    fn neighbor_spawn_friend_name(&self, target: &str) -> Option<String>;
    fn stamp_neighbor_message(
        &self,
        envelope: crate::service::messenger::MessageEnvelope,
    ) -> crate::service::messenger::MessageEnvelope;
    fn require_neighbor_workspace_target(
        &self,
        permission: &Permission,
        friendship: &str,
        target: &str,
    ) -> Result<(), String>;
    fn filter_neighbor_tab_listing(
        &self,
        caller: &TabCaller,
        listing: &mut Value,
    ) -> impl std::future::Future<Output = Result<bool, String>> + Send;
    fn authorize_neighbor_tab_message(
        &self,
        caller: &TabCaller,
        target: &str,
        plain_text: bool,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

impl NeighborsExt for ServiceContext {
    fn neighbor_spawn_friend_name(&self, target: &str) -> Option<String> {
        let friendship = self
            .mux
            .read()
            .get_tab(target)
            .and_then(|tab| tab.neighbor_authority.as_ref())
            .map(|authority| authority.friendship_id.clone())?;
        Some(
            self.neighbor_cell_names()
                .get(&friendship)
                .cloned()
                .unwrap_or_else(|| "Neighbor".into()),
        )
    }
    fn stamp_neighbor_message(
        &self,
        mut envelope: crate::service::messenger::MessageEnvelope,
    ) -> crate::service::messenger::MessageEnvelope {
        let authority = self
            .mux
            .read()
            .get_tab(&envelope.from_tab)
            .and_then(|tab| tab.neighbor_authority.clone());
        if let Some(authority) = authority {
            envelope.neighbor_friend = Some(
                self.neighbor_cell_names()
                    .get(&authority.friendship_id)
                    .cloned()
                    .unwrap_or_else(|| "Neighbor".into()),
            );
            envelope.from_neighbor = Some(authority.friendship_id);
        }
        envelope
    }

    fn require_neighbor_workspace_target(
        &self,
        permission: &Permission,
        friendship: &str,
        target: &str,
    ) -> Result<(), String> {
        Capability::TabSend.require(permission)?;
        let mux = self.mux.read();
        let permitted = mux
            .resolve_tab_id(target)
            .and_then(|id| {
                let tab = mux.get_tab(&id)?;
                let workspace = mux.find_workspace_for_tab(&id)?;
                Some(
                    !tab.hidden
                        && tab.agent_type.is_some()
                        && tab
                            .neighbor_authority
                            .as_ref()
                            .is_none_or(|authority| authority.friendship_id == friendship)
                        && permission
                            .workspaces
                            .as_ref()
                            .is_some_and(|ids| ids.contains(&workspace)),
                )
            })
            .unwrap_or(false);
        if permitted {
            Ok(())
        } else {
            Capability::Denied.require(permission)
        }
    }

    async fn filter_neighbor_tab_listing(
        &self,
        caller: &TabCaller,
        listing: &mut Value,
    ) -> Result<bool, String> {
        let Some((friendship, permission)) = self.neighbor_capability_permission(caller).await?
        else {
            return Ok(false);
        };
        Capability::TabList.require(&permission)?;
        let Some(rows) = listing.as_array_mut() else {
            *listing = serde_json::json!([]);
            return Ok(true);
        };
        let mux = self.mux.read();
        rows.retain_mut(|row| {
            let permitted = row["id"]
                .as_str()
                .and_then(|id| {
                    let tab = mux.get_tab(id)?;
                    let workspace = mux.find_workspace_for_tab(id)?;
                    Some(
                        !tab.hidden
                            && tab.agent_type.is_some()
                            && tab
                                .neighbor_authority
                                .as_ref()
                                .is_none_or(|authority| authority.friendship_id == friendship)
                            && permission
                                .workspaces
                                .as_ref()
                                .is_some_and(|ids| ids.contains(&workspace)),
                    )
                })
                .unwrap_or(false);
            if permitted {
                if let Some(row) = row.as_object_mut() {
                    row.retain(|key, _| {
                        matches!(
                            key.as_str(),
                            "id" | "tab_id"
                                | "title"
                                | "description"
                                | "workspace_id"
                                | "workspace_name"
                                | "agent_type"
                                | "agent_booting"
                                | "status"
                                | "agent_session_model"
                        )
                    });
                }
            }
            permitted
        });
        Ok(true)
    }

    async fn authorize_neighbor_tab_message(
        &self,
        caller: &TabCaller,
        target: &str,
        plain_text: bool,
    ) -> Result<(), String> {
        let Some((friendship, permission)) = self.neighbor_capability_permission(caller).await?
        else {
            return Ok(());
        };
        if !plain_text {
            return Capability::Denied.require(&permission);
        }
        self.require_neighbor_workspace_target(&permission, &friendship, target)
    }
}

#[cfg(test)]
pub mod tests;
