use codeterm_neighbors_service::service::cloud_installation::CloudInstallation;

pub(super) fn installation() -> Result<CloudInstallation, String> {
    let mesh = codeterm_service_core::mesh::peers::persisted_public_identity()
        .map_err(|_| "A persisted cloud Mesh identity is required")?;
    CloudInstallation::open(
        &codeterm_service_core::paths::data_dir(),
        mesh.instance_id,
        mesh.mesh_public_key,
    )
}

#[cfg(target_os = "linux")]
pub(super) fn report_ingress_ready(
    service: &codeterm_service_core::service::ServiceState,
    mesh: &codeterm_mesh::MeshState,
) {
    if std::env::var("DOMIOS_CLOUD_PLATFORM").as_deref() != Ok("fly_machines") {
        return;
    }
    let Some(installation) = service.try_ext::<CloudInstallation>().cloned() else {
        return;
    };
    let service = std::sync::Arc::downgrade(service);
    let mut gate = mesh.lock().gate_watch();
    tokio::spawn(async move {
        loop {
            gate.borrow_and_update();
            let Some(owner) = service.upgrade() else {
                return;
            };
            let Some(ingress) = owner.try_ext::<crate::cloud_ingress::CloudIngressHandle>() else {
                return;
            };
            let ready = ingress.enrolled_readiness();
            drop(owner);
            match ready.await {
                Ok(()) => match installation.record_ingress_ready().await {
                    Ok(Some(ready_at)) => {
                        log::debug!("[cloud] ingress readiness recorded at {ready_at}");
                        return;
                    }
                    Ok(None) => return,
                    Err(error) => log::debug!("[cloud] ingress readiness report failed: {error}"),
                },
                Err(error) => log::debug!("[cloud] ingress not ready: {}", error.code()),
            }
            if gate.changed().await.is_err() {
                return;
            }
        }
    });
}
