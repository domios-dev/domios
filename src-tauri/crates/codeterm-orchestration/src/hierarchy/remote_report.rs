use codeterm_service_core::{
    mesh::types::RelayCommand, service::mesh_artifacts_transfer_via, MeshStateHandle,
};

use super::ServiceContext;

impl HierarchyRemoteReportExt for ServiceContext {
    fn relay_report_to_peer(
        &self,
        peer: String,
        target_tab: String,
        from_title: String,
        from_tab: String,
        model: Option<String>,
        body: String,
    ) {
        let mesh = self.ext::<MeshStateHandle>().clone();
        let peer_id = mesh
            .lock()
            .resolve_peer(&peer)
            .ok()
            .map(|peer| peer.instance_id.clone());
        let Some(peer_id) = peer_id else {
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
        let mut cmd = RelayCommand {
            method: "POST".into(),
            path: format!("/api/tabs/{}/send", target_tab),
            body: send_body,
            // At-most-once: a report is one logical send; dedupe by this id so a
            // relay retry can never double-post it into the target tab.
            delivery_id: Some(uuid::Uuid::new_v4().to_string()),
        };
        let svc = self.clone();
        codeterm_service_core::async_runtime::spawn(async move {
            let send = |command: RelayCommand| {
                let client = mesh.lock().client_for(&peer_id);
                async move {
                    let client = client.ok_or("Report peer is unavailable")?;
                    client
                        .send_relay(&command)
                        .await
                        .map_err(|error| error.to_string())
                }
            };
            if let Err(error) = mesh_artifacts_transfer_via(
                &svc,
                Ok(Some(from_tab.clone())),
                &cmd.method,
                &cmd.path,
                &mut cmd.body,
                &send,
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
            match send(cmd).await {
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

pub trait HierarchyRemoteReportExt {
    fn relay_report_to_peer(
        &self,
        peer: String,
        target_tab: String,
        from_title: String,
        from_tab: String,
        model: Option<String>,
        body: String,
    );
}
