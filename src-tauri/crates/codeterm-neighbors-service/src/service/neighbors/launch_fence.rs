use super::ServiceContext;

pub use codeterm_service_core::service::neighbor_state::{begin, Mutation};

pub trait NeighborsExt {
    fn retire_neighbor_children(
        &self,
        root: &str,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
    fn retire_neighbor_children_sync(&self, root: &str) -> Result<(), String>;
    fn neighbor_launch_blocked(&self, friendship: &str) -> bool;
    fn begin_neighbor_mutation(
        &self,
        friendship: &str,
    ) -> impl std::future::Future<Output = Result<Mutation, String>> + Send;
    fn prepare_neighbor_root_close(&self, tab: &str) -> Result<Option<Mutation>, String>;
}

impl NeighborsExt for ServiceContext {
    async fn retire_neighbor_children(&self, root: &str) -> Result<(), String> {
        let root = root.to_owned();
        let service = self.clone();
        tokio::task::spawn_blocking(move || service.retire_neighbor_children_sync(&root))
            .await
            .map_err(|error| error.to_string())?
    }

    fn retire_neighbor_children_sync(&self, root: &str) -> Result<(), String> {
        let children: Vec<_> = self
            .mux
            .read()
            .snapshot()
            .tabs
            .iter()
            .filter(|(_, tab)| {
                tab.neighbor_authority
                    .as_ref()
                    .is_some_and(|child| child.root_tab_id.as_deref() == Some(root))
            })
            .map(|(id, _)| id.clone())
            .collect();
        for child in children {
            if self.mux.read().get_tab(&child).is_some() {
                self.close_tab(&child)?;
            }
        }
        Ok(())
    }
    fn neighbor_launch_blocked(&self, friendship: &str) -> bool {
        self.neighbor_launches()
            .mutations
            .lock()
            .contains_key(friendship)
    }

    async fn begin_neighbor_mutation(&self, friendship: &str) -> Result<Mutation, String> {
        let state = self.neighbor_launches().clone();
        let friendship = friendship.to_owned();
        tokio::task::spawn_blocking(move || begin(state, friendship))
            .await
            .map_err(|error| error.to_string())
    }

    fn prepare_neighbor_root_close(&self, tab: &str) -> Result<Option<Mutation>, String> {
        let root = {
            let mux = self.mux.read();
            mux.resolve_tab_id(tab).and_then(|id| {
                mux.get_tab(&id)
                    .and_then(|tab| tab.neighbor_authority.as_ref())
                    .filter(|authority| authority.root_tab_id.is_none())
                    .cloned()
                    .map(|authority| (id, authority))
            })
        };
        let Some((root, authority)) = root else {
            return Ok(None);
        };
        let mutation = begin(
            self.neighbor_launches().clone(),
            authority.friendship_id.clone(),
        );
        self.retire_neighbor_children_sync(&root)?;
        Ok(Some(mutation))
    }
}
