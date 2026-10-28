use super::ServiceContext;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub(super) struct State {
    pub(super) gate: parking_lot::Mutex<()>,
    mutations: parking_lot::Mutex<HashMap<String, usize>>,
}

pub(crate) struct Mutation {
    state: Arc<State>,
    friendship: String,
}

impl Drop for Mutation {
    fn drop(&mut self) {
        let mut mutations = self.state.mutations.lock();
        if let Some(count) = mutations.get_mut(&self.friendship) {
            *count -= 1;
            if *count == 0 {
                mutations.remove(&self.friendship);
            }
        }
    }
}

fn begin(state: Arc<State>, friendship: String) -> Mutation {
    {
        // Finish any launch before collecting descendants; new launches then see the mutation.
        let _gate = state.gate.lock();
        *state
            .mutations
            .lock()
            .entry(friendship.clone())
            .or_default() += 1;
    }
    Mutation { state, friendship }
}

impl ServiceContext {
    pub(super) async fn retire_neighbor_children(&self, root: &str) -> Result<(), String> {
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
            .filter_map(|(id, tab)| {
                tab.neighbor_authority
                    .as_ref()
                    .is_some_and(|child| child.root_tab_id.as_deref() == Some(root))
                    .then(|| id.clone())
            })
            .collect();
        for child in children {
            if self.mux.read().get_tab(&child).is_some() {
                self.close_tab(&child)?;
            }
        }
        Ok(())
    }
    pub(super) fn neighbor_launch_blocked(&self, friendship: &str) -> bool {
        self.neighbor_cell()
            .launches
            .mutations
            .lock()
            .contains_key(friendship)
    }

    pub(super) async fn begin_neighbor_mutation(
        &self,
        friendship: &str,
    ) -> Result<Mutation, String> {
        let state = self.neighbor_cell().launches.clone();
        let friendship = friendship.to_owned();
        tokio::task::spawn_blocking(move || begin(state, friendship))
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) fn prepare_neighbor_root_close(
        &self,
        tab: &str,
    ) -> Result<Option<Mutation>, String> {
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
            self.neighbor_cell().launches.clone(),
            authority.friendship_id.clone(),
        );
        self.retire_neighbor_children_sync(&root)?;
        Ok(Some(mutation))
    }
}
