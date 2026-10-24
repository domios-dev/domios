use codeterm_mesh::account_mesh::{
    approval::{LinkDecision, LinkRequest},
    protocol::LinkApproval,
    AdmissionRefusal,
};
use codeterm_notification_core::contract::{NotificationAction, NotifyLevel};

use super::super::notification::{NotificationOrigin, PushIntent};
use crate::notification::{answer::AnswerAdmission, store::Ask};
use crate::service::ServiceContext;

fn approval_ask(request: &LinkRequest) -> Ask {
    Ask {
        daemon_request: Some(format!("mesh-link:{}", request.request_id)),
        actions: Some(
            ["Approve", "Decline"]
                .into_iter()
                .map(|label| NotificationAction::Answer {
                    label: label.into(),
                })
                .collect(),
        ),
        ..Ask::default()
    }
}

impl ServiceContext {
    pub fn start_mesh_link(&self, installation_id: String) -> Result<String, AdmissionRefusal> {
        let mesh = self
            .try_ext::<crate::MeshStateHandle>()
            .cloned()
            .ok_or(AdmissionRefusal::Offline)?;
        let service = self
            .weak_self()
            .upgrade()
            .ok_or(AdmissionRefusal::Offline)?;
        let request_id = mesh.lock().begin_outgoing_link(&installation_id)?;
        self.ui
            .emit("account-mesh-changed", serde_json::Value::Null);
        let outgoing_id = request_id.clone();
        tokio::spawn(async move {
            let result =
                codeterm_mesh::account_mesh::link::link_installation(&mesh, &installation_id).await;
            mesh.lock().finish_outgoing_link(&outgoing_id, &result);
            service
                .ui
                .emit("account-mesh-changed", serde_json::Value::Null);
        });
        Ok(request_id)
    }

    pub async fn receive_mesh_link_approval(
        &self,
        proof: LinkApproval,
    ) -> Result<(), AdmissionRefusal> {
        let mesh = self
            .try_ext::<crate::MeshStateHandle>()
            .ok_or(AdmissionRefusal::Offline)?;
        let request = mesh.lock().request_link_approval(&proof)?;
        let id = self
            .create_notification_actions_reported(
                format!("{} wants to connect", request.name),
                Some("Approve this connection on this machine.".into()),
                Some("info"),
                approval_ask(&request),
                Some(true),
                NotificationOrigin::default(),
                PushIntent {
                    level: Some(NotifyLevel::ActionNeeded),
                    event_key: Some(format!("mesh-link:{}", request.request_id)),
                },
                None,
            )
            .0;
        mesh.lock()
            .set_link_notification(&request.request_id, id.clone());
        self.ui
            .emit("account-mesh-changed", serde_json::Value::Null);
        let result = codeterm_mesh::account_mesh::approval::wait_for_link_approval(
            mesh,
            &request.request_id,
        )
        .await;
        if result.is_err() {
            mesh.lock().clear_link_request(&request.request_id);
        }
        let resolved = self
            .notif
            .lock()
            .resolve_daemon_request(&format!("mesh-link:{}", request.request_id));
        self.announce_resolved(&resolved);
        self.ui
            .emit("account-mesh-changed", serde_json::Value::Null);
        result
    }

    pub(crate) fn answer_mesh_link_notification(
        &self,
        id: &str,
        label: &str,
    ) -> Option<Result<serde_json::Value, String>> {
        let slot = self.notif.lock().answer_slot(id)?.clone();
        let request = slot.daemon_request?;
        let request_id = request.strip_prefix("mesh-link:")?;
        if slot.delivered {
            if let Some(standing) = slot.answered {
                return Some(Ok(
                    serde_json::json!({ "answered": standing, "delivered": true }),
                ));
            }
        }
        Some((|| {
            let approve = match label {
                "Approve" => true,
                "Decline" => false,
                _ => return Err("That choice was not offered".into()),
            };
            let mesh = self
                .try_ext::<crate::MeshStateHandle>()
                .ok_or("Mesh is unavailable")?;
            let mut manager = mesh.lock();
            let mut store = self.notif.lock();
            if !store.notification(id).is_some_and(|record| {
                !record.resolved
                    && record.actions.as_deref().is_some_and(|actions| {
                        actions
                            .iter()
                            .any(|action| action.answer_label() == Some(label))
                    })
            }) {
                return Err("This connection request is no longer available".into());
            }
            let decision = manager
                .decide_link(request_id, approve)
                .map_err(|error| error.to_string())?;
            let standing = if decision == LinkDecision::Approved {
                "Approve"
            } else {
                "Decline"
            };
            let admission = store.record_answer(id, standing);
            if !matches!(
                admission,
                AnswerAdmission::Recorded(_) | AnswerAdmission::AlreadyAnswered(_)
            ) {
                return Err("This connection request is no longer available".into());
            }
            store.mark_answer_delivered(id);
            drop(store);
            drop(manager);
            self.ui
                .emit("account-mesh-changed", serde_json::Value::Null);
            self.notify_event(
                "notification-answered",
                serde_json::json!({ "id": id, "answered": standing, "delivered": true }),
            );
            Ok(serde_json::json!({ "answered": standing, "delivered": true }))
        })())
    }
}

#[cfg(test)]
mod tests;
