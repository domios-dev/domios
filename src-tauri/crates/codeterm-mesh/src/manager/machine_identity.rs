use super::MeshManager;
use crate::machine_identity::{self, MachineIdentity, Revision, SignedMachineIdentity};
use crate::MeshState;

impl MeshManager {
    pub(super) fn load_machine_identity(&mut self) {
        if let Some(saved) = machine_identity::load(
            &crate::peers::mesh_dir().join("machine-identity.json"),
            &self.ident.identity.instance_id,
        ) {
            self.machine_identity = saved;
        }
    }

    pub fn machine_identity(&self) -> MachineIdentity {
        self.machine_identity.identity
    }

    pub fn machine_identity_chosen(&self) -> bool {
        self.machine_identity.number > 0
    }

    pub fn machine_identity_revision(&self) -> u64 {
        self.machine_identity.number
    }

    pub fn peer_machine_identity(&self, instance_id: &str) -> Option<MachineIdentity> {
        self.peer_machine_identities
            .get(instance_id)
            .map(|revision| revision.identity)
    }

    pub(super) fn sign_machine_identity(&self) -> Option<SignedMachineIdentity> {
        SignedMachineIdentity::sign(
            &self.ident.identity.instance_id,
            self.ident.identity.private_key.as_deref()?,
            self.machine_identity,
        )
    }

    pub(super) fn accept_machine_identity(
        &mut self,
        sender_id: &str,
        signed: &SignedMachineIdentity,
    ) -> bool {
        let Some(peer) = self.paired_peers.get(sender_id) else {
            return false;
        };
        let Some(revision) = signed.verify(sender_id, &peer.public_key) else {
            return false;
        };
        if self
            .peer_machine_identities
            .get(sender_id)
            .is_some_and(|known| known.number >= revision.number)
        {
            return false;
        }
        self.peer_machine_identities
            .insert(sender_id.to_owned(), revision);
        true
    }

    pub fn apply_machine_identity_sync(
        &mut self,
        sender_id: &str,
        peers: &[crate::types::GossipPeerInfo],
    ) -> bool {
        self.traffic_open(sender_id)
            && peers
                .iter()
                .find(|peer| peer.instance_id == sender_id)
                .and_then(|peer| peer.machine_identity.as_ref())
                .is_some_and(|signed| self.accept_machine_identity(sender_id, signed))
    }
}

pub fn set_machine_identity(
    mesh: &MeshState,
    identity: MachineIdentity,
    expected_revision: Option<u64>,
) -> Result<bool, String> {
    let writer = mesh.lock().machine_identity_writer.clone();
    let _guard = writer.lock();
    let (instance_id, previous, persist) = {
        let manager = mesh.lock();
        (
            manager.ident.identity.instance_id.clone(),
            manager.machine_identity,
            manager.disk_persist,
        )
    };
    if previous.number > 0 && previous.identity == identity {
        return Ok(false);
    }
    if expected_revision.is_some_and(|expected| expected != previous.number) {
        return Err("The machine's icon and colour changed after this request".into());
    }
    let revision = Revision {
        identity,
        number: previous
            .number
            .checked_add(1)
            .ok_or("Machine identity revision exhausted")?,
    };
    if persist {
        machine_identity::save(
            &crate::peers::mesh_dir().join("machine-identity.json"),
            &instance_id,
            revision,
        )?;
    }
    mesh.lock().machine_identity = revision;
    Ok(true)
}
