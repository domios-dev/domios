use std::sync::{Mutex, OnceLock};

use crate::ports::host::UiEvents;
use std::sync::Arc;

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
    ui: Arc<dyn UiEvents>,
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
        ui.emit("mesh-state-changed", serde_json::Value::Null);
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
    ui: Arc<dyn UiEvents>,
    mesh_state: MeshStateHandle,
    api_port: u16,
    https_port: Option<u16>,
) -> Result<mesh::ServeResult, String> {
    mutate(ui, mesh_state, api_port, Mutation::Enable(https_port)).await
}

pub async fn disable(
    ui: Arc<dyn UiEvents>,
    mesh_state: MeshStateHandle,
    api_port: u16,
) -> Result<mesh::ServeResult, String> {
    mutate(ui, mesh_state, api_port, Mutation::Disable).await
}

pub fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

pub fn uploads_dir() -> std::path::PathBuf {
    crate::paths::data_dir().join("uploads")
}

pub const PEBBLE_WEBHOOK_PATH: &str = "/api/plugins/pebble/webhook";

pub const PEBBLE_WEBHOOK_SECRET: &str = "pebble_webhook_secret";
