use super::*;
use std::sync::Arc;

impl ServiceContext {
    pub fn acknowledge_remote_worker_ack(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        worker_tab_id: &str,
    ) {
        let identity =
            remote_worker_ack_identity(plan_id, task_id, attempt_id, peer_id, worker_tab_id);
        self.worker_ack.acknowledge(&identity);
    }

    pub fn arm_remote_worker_ack_deadline(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        peer_id: &str,
        peer_name: &str,
        worker_tab_id: &str,
        parent_tab_id: &str,
    ) {
        let identity =
            remote_worker_ack_identity(plan_id, task_id, attempt_id, peer_id, worker_tab_id);
        let arm = self.worker_ack.arm(identity.clone());
        let Some(receiver) = arm.receiver.filter(|_| arm.starts_supervisor) else {
            return;
        };
        let service = Arc::new(self.clone());
        let plan_id = plan_id.to_string();
        let task_id = task_id.to_string();
        let attempt_id = attempt_id.to_string();
        let peer_id = peer_id.to_string();
        let peer_name = peer_name.to_string();
        let worker_tab_id = worker_tab_id.to_string();
        let parent_tab_id = parent_tab_id.to_string();
        std::thread::spawn(move || {
            if await_remote_worker_ack(&receiver, REMOTE_WORKER_ACK_DEADLINE) {
                return;
            }
            if !service.worker_ack.expire(&identity) {
                return;
            }
            let peer_online = service
                .ext::<crate::MeshStateHandle>()
                .lock()
                .paired_peers
                .get(&peer_id)
                .is_some_and(|peer| peer.online);
            let finalized = if peer_online {
                service.plan_store.mark_remote_ack_timeout(
                    &plan_id,
                    &task_id,
                    &attempt_id,
                    &peer_id,
                    &worker_tab_id,
                    epoch_ms(),
                )
            } else {
                service.plan_store.mark_remote_peer_offline_before_ack(
                    &plan_id,
                    &task_id,
                    &attempt_id,
                    &peer_id,
                    &worker_tab_id,
                    epoch_ms(),
                )
            }
            .unwrap_or(false);
            if !finalized {
                return;
            }
            service.ui.emit(
                "plan-changed",
                serde_json::to_value(json!({"plan_id": plan_id})).unwrap_or_default(),
            );
            let payload = json!({
                "kind": if peer_online { "remote_ack_timeout" } else { "remote_peer_offline" },
                "plan_id": plan_id,
                "task_id": task_id,
                "attempt_id": attempt_id,
                "peer_id": peer_id,
                "peer_name": peer_name,
                "tab_id": worker_tab_id,
                "impact": if peer_online {
                    "Remote Worker did not acknowledge before the origin deadline; the task is dispatchable again."
                } else {
                    "Remote peer went offline before Worker ACK; the task is dispatchable again."
                },
            });
            service.publish_worker_lifecycle_report(
                &parent_tab_id,
                &worker_tab_id,
                &format!("Worker@{peer_name}"),
                &plan_id,
                &task_id,
                payload,
            );
        });
    }
}

pub fn remote_worker_ack_identity(
    plan_id: &str,
    task_id: &str,
    attempt_id: &str,
    peer_id: &str,
    worker_tab_id: &str,
) -> WorkerAckIdentity {
    WorkerAckIdentity {
        plan_id: plan_id.to_string(),
        task_id: task_id.to_string(),
        attempt_id: attempt_id.to_string(),
        tab_id: format!("remote:{peer_id}:{worker_tab_id}"),
        startup_attempt_id: None,
        session_id: None,
        binding_nonce: None,
    }
}
