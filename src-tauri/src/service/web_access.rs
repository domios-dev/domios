use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;

use crate::{mesh, MeshStateHandle};

enum Mutation {
    Enable(Option<u16>),
    Disable,
}

fn mutation_gate() -> &'static Mutex<()> {
    static GATE: OnceLock<Mutex<()>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(()))
}

async fn mutate(
    app: AppHandle,
    mesh_state: MeshStateHandle,
    api_port: u16,
    mutation: Mutation,
) -> Result<mesh::ServeResult, String> {
    let mesh_for_mutation = mesh_state.clone();
    let (mut result, changed) = tokio::task::spawn_blocking(move || {
        let _guard = mutation_gate()
            .lock()
            .map_err(|_| "Web Access mutation lock is poisoned".to_string())?;
        let result = match mutation {
            Mutation::Enable(https_port) => mesh::enable_web_serve(api_port, https_port),
            Mutation::Disable => mesh::disable_web_serve(api_port),
        };
        let changed = result.ok && mesh::refresh_local_addresses_blocking_force(&mesh_for_mutation);
        Ok::<_, String>((result, changed))
    })
    .await
    .map_err(|error| format!("Web Access worker failed: {error}"))??;

    if changed {
        let report = mesh::broadcast_announce(&mesh_state, None).await;
        crate::api::events::emit_everywhere(&app, "mesh-state-changed", serde_json::Value::Null);
        if !report.errors.is_empty() {
            let publication_warning = format!(
                "Web Access changed locally, but {} Mesh peer announcement(s) failed",
                report.errors.len()
            );
            result.warning = Some(match result.warning.take() {
                Some(existing) => format!("{existing}. {publication_warning}"),
                None => publication_warning,
            });
        }
    }

    Ok(result)
}

pub async fn enable(
    app: AppHandle,
    mesh_state: MeshStateHandle,
    api_port: u16,
    https_port: Option<u16>,
) -> Result<mesh::ServeResult, String> {
    mutate(app, mesh_state, api_port, Mutation::Enable(https_port)).await
}

pub async fn disable(
    app: AppHandle,
    mesh_state: MeshStateHandle,
    api_port: u16,
) -> Result<mesh::ServeResult, String> {
    mutate(app, mesh_state, api_port, Mutation::Disable).await
}
