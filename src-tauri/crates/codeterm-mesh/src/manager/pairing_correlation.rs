use super::MeshManager;
use crate::types::PairedPeer;

impl MeshManager {
    pub fn complete_pairing_accept(
        &mut self,
        request_id: &str,
        peer: PairedPeer,
    ) -> Result<(), String> {
        self.validate_pairing_accept_identity(request_id, &peer.instance_id)?;
        self.complete_explicit_pairing(peer);
        self.pending_requests.remove(request_id);
        self.pairing_targets.remove(request_id);
        let delivery_key = self
            .pairing_deliveries
            .iter()
            .find(|(_, delivery)| delivery.request_id() == request_id)
            .map(|(target_id, _)| target_id.clone());
        if let Some(delivery) =
            delivery_key.and_then(|target_id| self.pairing_deliveries.remove(&target_id))
        {
            delivery.complete(Ok(()));
        }
        Ok(())
    }
}
