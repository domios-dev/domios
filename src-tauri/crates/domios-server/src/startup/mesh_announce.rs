use std::collections::BTreeSet;
use std::net::SocketAddr;

use codeterm_service_core::mesh::MeshManager;

#[derive(Default)]
pub(super) struct Readiness {
    listeners: BTreeSet<SocketAddr>,
    peers: BTreeSet<String>,
}

impl Readiness {
    pub(super) fn snapshot(mesh: &MeshManager, enabled: bool) -> Self {
        let online = enabled && mesh.build_announce().online;
        Self {
            listeners: mesh.ready_listeners.iter().copied().collect(),
            peers: mesh
                .paired_peers
                .values()
                .filter(|peer| {
                    online && !peer.addresses.is_empty() && mesh.announce_allowed(&peer.instance_id)
                })
                .map(|peer| peer.instance_id.clone())
                .collect(),
        }
    }

    pub(super) fn update(&mut self, next: Self) -> bool {
        let announce = !next.peers.is_subset(&self.peers)
            || (self.listeners != next.listeners
                && (!self.peers.is_empty() || !next.peers.is_empty()));
        *self = next;
        announce
    }
}

#[cfg(test)]
mod tests;
