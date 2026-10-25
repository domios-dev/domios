use super::{AccountMeshRefusal, ServiceContext};
use std::sync::Weak;

pub(super) async fn notify(service: &Weak<ServiceContext>, revision: u64) {
    let Some(service) = service.upgrade() else {
        return;
    };
    if !service.configuration().mesh.enabled {
        return;
    }
    if let Some(mesh) = service.try_ext::<crate::MeshStateHandle>().cloned() {
        codeterm_mesh::presence_notice::broadcast_presence_notice(&mesh, revision).await;
    }
}

impl ServiceContext {
    pub async fn account_mesh_presence_notice(
        &self,
        sender: &str,
        nonce: &str,
        revision: u64,
    ) -> Result<bool, String> {
        let service = self.account_mesh().await?;
        service
            .mesh
            .presence_notice(sender, nonce, revision)
            .await
            .map_err(|refusal: AccountMeshRefusal| {
                format!("Account registry refresh refused: {refusal:?}")
            })
    }
}
