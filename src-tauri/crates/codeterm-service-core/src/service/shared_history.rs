use crate::service::plan::shared_checkout::{LostSharedCommit, SharedHistoryPorts};
use crate::service::plan::PlanError;
use crate::service::team_core::now_ms;
use crate::service::ServiceContext;
use std::path::Path;
use std::sync::Arc;

impl ServiceContext {
    /// Binds incident delivery and watch retention, then reconciles each shared checkout once.
    pub fn restore_shared_history_watches(&self) -> Result<(), PlanError> {
        let delivery = self.clone();
        let watcher = self.clone();
        self.plan_store
            .bind_shared_history_ports(SharedHistoryPorts {
                lost: Arc::new(move |incident| {
                    let service = delivery.clone();
                    let incident = incident.clone();
                    crate::async_runtime::spawn_blocking(move || {
                        service.deliver_lost_shared_commit(&incident)
                    });
                }),
                watch: Arc::new(move |checkout| watcher.retain_shared_history_watch(checkout)),
            });
        for checkout in self.plan_store.shared_history_checkouts()? {
            self.retain_shared_history_watch(&checkout);
            if let Err(error) = self
                .plan_store
                .detect_lost_shared_commits(&checkout, now_ms())
            {
                log::warn!("[shared-history] cannot inspect {checkout}: {error}");
            }
        }
        Ok(())
    }

    /// Repository watch signal: HEAD or a ref moved in `identity`.
    pub fn observe_repository_head(&self, identity: &str) {
        let service = self.clone();
        let identity = identity.to_string();
        crate::async_runtime::spawn_blocking(move || {
            if let Err(error) = service
                .plan_store
                .detect_lost_shared_commits(&identity, now_ms())
            {
                log::warn!("[shared-history] cannot inspect {identity}: {error}");
            }
        });
    }

    fn retain_shared_history_watch(&self, checkout: &str) {
        if let Err(error) = self
            .ext::<crate::repowatch::RepoWatchRegistryHandle>()
            .retain_lane_watch(&format!("shared-history:{checkout}"), Path::new(checkout))
        {
            log::debug!("[shared-history] repository watch unavailable: {error}");
        }
    }

    fn deliver_lost_shared_commit(&self, incident: &LostSharedCommit) {
        self.emit_committed_plan_changed(&incident.plan_id);
        let owner = match self
            .plan_store
            .notice_delivery_target_projection(&incident.plan_id)
        {
            Ok(target) => target.owner_tab_id,
            Err(error) => {
                log::warn!(
                    "[shared-history] no notice target for {}: {error}",
                    incident.plan_id
                );
                return;
            }
        };
        let Some(owner) = owner else {
            return;
        };
        let text = format!(
            "[shared checkout] Reported commit {} of task {} (attempt {}) is no longer on the shared checkout HEAD: {} -> {} (likely {}). Domios does not restore history; recover the commit from the reflog or move the task to a lane.",
            incident.commit,
            incident.task_id,
            incident.attempt_id,
            incident.old_head.as_deref().unwrap_or("unknown"),
            incident.new_head,
            incident.cause,
        );
        if let Err(error) = crate::service::orchestrator_digests::deliver_orchestrator_notice_exact(
            self,
            &incident.plan_id,
            &owner,
            "plan",
            &text,
        ) {
            log::warn!(
                "[shared-history] notice for {} not delivered: {error}",
                incident.plan_id
            );
        }
    }
}
