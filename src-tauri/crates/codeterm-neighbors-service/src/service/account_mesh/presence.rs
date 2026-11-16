use super::{AccountMeshRefusal, ServiceContext};
use crate::prelude::*;
use std::sync::Weak;

pub async fn notify(service: &Weak<ServiceContext>, revision: u64) {
    let Some(service) = service.upgrade() else {
        return;
    };
    if !service.configuration().mesh.enabled {
        return;
    }
    if let Some(mesh) = service.try_ext::<crate::MeshStateHandle>().cloned() {
        let snapshot = mesh.lock().signed_registry.clone();
        if let Some(snapshot) = snapshot {
            codeterm_mesh::presence_notice::broadcast_registry_snapshot(&mesh, &snapshot).await;
        } else {
            codeterm_mesh::presence_notice::broadcast_presence_notice(&mesh, revision).await;
        }
    }
}

pub trait NeighborsExt {
    fn account_mesh_presence_notice(
        &self,
        sender: &str,
        nonce: &str,
        revision: u64,
    ) -> impl std::future::Future<Output = Result<bool, String>> + Send;
}

impl NeighborsExt for ServiceContext {
    async fn account_mesh_presence_notice(
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

pub async fn notify_registry(service: &Weak<ServiceContext>) {
    let Some(service) = service.upgrade() else {
        return;
    };
    if !service.configuration().mesh.enabled {
        return;
    }
    if let Some(mesh) = service.try_ext::<crate::MeshStateHandle>().cloned() {
        let snapshot = mesh.lock().signed_registry.clone();
        if let Some(snapshot) = snapshot {
            codeterm_mesh::presence_notice::broadcast_registry_snapshot(&mesh, &snapshot).await;
        }
    }
}

pub trait NeighborsAdditionalExt {
    fn account_mesh_registry_snapshot(
        &self,
        snapshot: &codeterm_entitlements::DetachedGrant,
    ) -> impl std::future::Future<Output = Result<bool, String>> + Send;
}

impl NeighborsAdditionalExt for ServiceContext {
    async fn account_mesh_registry_snapshot(
        &self,
        snapshot: &codeterm_entitlements::DetachedGrant,
    ) -> Result<bool, String> {
        let service = self.account_mesh().await?;
        match service.mesh.receive_registry_snapshot(snapshot).await {
            Ok(applied) => Ok(applied),
            Err(error) => Err(format!("Signed registry rejected: {error:?}")),
        }
    }
}
