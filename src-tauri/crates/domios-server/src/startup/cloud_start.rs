use codeterm_neighbors_service::service::cloud_installation::CloudInstallation;

pub(crate) fn enabled() -> bool {
    std::env::var_os("DOMIOS_CLOUD_MACHINE").is_some()
}

pub(super) fn installation() -> Result<CloudInstallation, String> {
    let mesh = codeterm_service_core::mesh::peers::persisted_public_identity()
        .map_err(|_| "A persisted cloud Mesh identity is required")?;
    CloudInstallation::open(
        &codeterm_service_core::paths::data_dir(),
        mesh.instance_id,
        mesh.mesh_public_key,
    )
}
