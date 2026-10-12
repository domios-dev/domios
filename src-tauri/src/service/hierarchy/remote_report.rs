use super::{resolve_peer_client_by_name, ServiceContext};
use tauri::Manager;

impl ServiceContext {
    pub(super) fn relay_report_to_peer(
        &self,
        peer: String,
        target_tab: String,
        from_title: String,
        from_tab: String,
        model: Option<String>,
        body: String,
    ) {
        let client = {
            let mesh_handle = self.app_handle.state::<crate::MeshStateHandle>();
            let mesh = mesh_handle.lock();
            resolve_peer_client_by_name(&mesh, &peer)
        };
        let Some(client) = client else {
            log::warn!(
                "[report/mesh] no paired peer named '{}' — report from {} stored only",
                peer,
                from_tab
            );
            return;
        };
        let mut send_body = serde_json::json!({
            "text": body,
            "source_tab": from_tab,
            "source_label": from_title,
            "kind": "report",
            "enter": true,
        });
        if let Some(m) = model.filter(|m| !m.trim().is_empty()) {
            send_body["source_model"] = serde_json::json!(m);
        }
        let mut cmd = crate::mesh::types::RelayCommand {
            method: "POST".into(),
            path: format!("/api/tabs/{}/send", target_tab),
            body: send_body,
            // At-most-once: a report is one logical send; dedupe by this id so a
            // relay retry can never double-post it into the target tab.
            delivery_id: Some(uuid::Uuid::new_v4().to_string()),
        };
        tauri::async_runtime::spawn(async move {
            if let Err(error) = crate::service::mesh_artifacts::transfer(
                &client,
                &cmd.method,
                &cmd.path,
                &mut cmd.body,
            )
            .await
            {
                log::warn!(
                    "[report/mesh] artifact transfer to {} failed: {}",
                    peer,
                    error
                );
                return;
            }
            match client.send_relay(&cmd).await {
                Ok(r) if r.ok => log::info!("[report/mesh] delivered to {}/{}", peer, target_tab),
                Ok(r) => log::warn!(
                    "[report/mesh] {} rejected report to {}: {:?}",
                    peer,
                    target_tab,
                    r.error
                ),
                Err(e) => log::warn!("[report/mesh] relay to {} failed: {}", peer, e),
            }
        });
    }
}
