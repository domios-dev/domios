use crate::service::ServiceContext;

impl ServiceContext {
    pub async fn disconnect_mesh_peer(&self, instance_id: &str) -> Result<(), String> {
        let mesh = self.try_ext::<crate::MeshStateHandle>().ok_or("Mesh is unavailable")?;
        let (peer, notice, token) = {
            let mut manager = mesh.lock();
            let token = manager.sign_request()?;
            let peer = manager.unpair(instance_id)?;
            let notice = manager.saved_disconnect(instance_id).map(|saved| saved.notice.clone());
            (peer, notice, token)
        };
        self.ui.emit("mesh-state-changed", serde_json::Value::Null);
        if let (Some(peer), Some(notice)) = (peer, notice) {
            codeterm_mesh::disconnect::deliver_disconnect(peer, notice, token).await;
        }
        Ok(())
    }
}
