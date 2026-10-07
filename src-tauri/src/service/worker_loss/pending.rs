use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use codeterm_types::notification::NotifyLevel;
use parking_lot::Mutex;

use super::{claim_report, log_loss_delivery, release_claim, WorkerLoss};
use crate::service::message_gateway::{DeliverySeam, MessageGateway};
use crate::service::notification::{NotificationOrigin, PushIntent};
use crate::service::ServiceContext;

pub(super) const WITHHELD_NO_AGENT: &str = "withheld_no_agent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LossRoute {
    Delivered,
    Withheld { first: bool },
    Failed(String),
}

impl LossRoute {
    pub(super) fn label(&self) -> String {
        match self {
            Self::Delivered => "delivered".to_string(),
            Self::Withheld { .. } => WITHHELD_NO_AGENT.to_string(),
            Self::Failed(error) => format!("failed: {error}"),
        }
    }

    fn opens_alert(&self) -> bool {
        matches!(self, Self::Withheld { first: true })
    }
}

#[derive(Default)]
pub(super) struct PendingWorkerLosses {
    by_orchestrator: HashMap<String, Vec<WorkerLoss>>,
    alerted: HashSet<String>,
}

impl PendingWorkerLosses {
    /// True when this loss opens a withheld episode for `orchestrator`, which owes one alert.
    fn withhold(&mut self, orchestrator: &str, loss: WorkerLoss) -> bool {
        let key = loss.dedupe_key();
        let held = self
            .by_orchestrator
            .entry(orchestrator.to_string())
            .or_default();
        held.retain(|pending| pending.dedupe_key() != key);
        held.push(loss);
        self.alerted.insert(orchestrator.to_string())
    }

    fn forget(&mut self, orchestrator: &str, loss: &WorkerLoss) {
        let key = loss.dedupe_key();
        if let Some(held) = self.by_orchestrator.get_mut(orchestrator) {
            held.retain(|pending| pending.dedupe_key() != key);
        }
        self.settle(orchestrator);
    }

    fn take(&mut self, orchestrator: &str) -> Vec<WorkerLoss> {
        self.by_orchestrator
            .remove(orchestrator)
            .unwrap_or_default()
    }

    fn settle(&mut self, orchestrator: &str) {
        if self
            .by_orchestrator
            .get(orchestrator)
            .is_none_or(Vec::is_empty)
        {
            self.by_orchestrator.remove(orchestrator);
            self.alerted.remove(orchestrator);
        }
    }

    fn has_pending(&self, orchestrator: &str) -> bool {
        self.by_orchestrator.contains_key(orchestrator)
    }

    pub(super) fn pending_for(&self, orchestrator: &str) -> Vec<WorkerLoss> {
        self.by_orchestrator
            .get(orchestrator)
            .cloned()
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn alert_open(&self, orchestrator: &str) -> bool {
        self.alerted.contains(orchestrator)
    }
}

pub(super) fn pending_worker_losses() -> &'static Mutex<PendingWorkerLosses> {
    static PENDING: OnceLock<Mutex<PendingWorkerLosses>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(PendingWorkerLosses::default()))
}

/// Submits one claimed loss; a zero-write refusal releases the claim and holds the loss for replay.
pub(super) fn route_loss<S: DeliverySeam>(
    gateway: &MessageGateway,
    seam: &S,
    pending: &Mutex<PendingWorkerLosses>,
    owner: &str,
    loss: &WorkerLoss,
) -> LossRoute {
    match gateway.submit(seam, loss.outbound_message(owner)) {
        Ok(_) => {
            pending.lock().forget(owner, loss);
            LossRoute::Delivered
        }
        Err(error) if error.is_agent_unavailable() => {
            release_claim(loss);
            let first = pending.lock().withhold(owner, loss.clone());
            LossRoute::Withheld { first }
        }
        Err(error) => LossRoute::Failed(error.to_string()),
    }
}

pub(super) fn replay_losses<S: DeliverySeam>(
    gateway: &MessageGateway,
    seam: &S,
    pending: &Mutex<PendingWorkerLosses>,
    owner: &str,
) -> Vec<(WorkerLoss, LossRoute)> {
    let has_pending = pending.lock().has_pending(owner);
    if !has_pending || !seam.accepts_system_notice(owner) {
        return Vec::new();
    }
    let held = pending.lock().take(owner);
    let mut routed = Vec::with_capacity(held.len());
    for loss in held {
        if !claim_report(&loss) {
            continue;
        }
        let route = route_loss(gateway, seam, pending, owner, &loss);
        routed.push((loss, route));
    }
    pending.lock().settle(owner);
    routed
}

impl ServiceContext {
    pub(super) fn route_worker_loss(&self, owner: &str, loss: &WorkerLoss) -> LossRoute {
        route_loss(&self.gateway, self, pending_worker_losses(), owner, loss)
    }

    pub(crate) fn replay_pending_worker_losses_if_routable(&self, tab_id: &str) {
        let routed = replay_losses(&self.gateway, self, pending_worker_losses(), tab_id);
        let mut alert = false;
        for (loss, route) in &routed {
            alert |= route.opens_alert();
            log_loss_delivery(loss, &format!("replay_{}", route.label()));
        }
        if alert {
            self.alert_withheld_worker_losses(tab_id);
        }
    }

    pub(super) fn alert_withheld_worker_losses_if_opened(&self, routed: &[(String, LossRoute)]) {
        let mut alerted: Vec<&str> = Vec::new();
        for (owner, route) in routed {
            if route.opens_alert() && !alerted.contains(&owner.as_str()) {
                alerted.push(owner.as_str());
                self.alert_withheld_worker_losses(owner);
            }
        }
    }

    fn alert_withheld_worker_losses(&self, orchestrator: &str) {
        let held = pending_worker_losses().lock().pending_for(orchestrator);
        if held.is_empty() {
            return;
        }
        let tasks = held
            .iter()
            .map(|loss| {
                format!(
                    "{} \"{}\" of plan {} (worker {})",
                    loss.task_id, loss.task_name, loss.plan_id, loss.worker_tab_id
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        self.create_notification_ext(
            "Worker losses withheld: Orchestrator has no live agent".into(),
            Some(format!(
                "Orchestrator Tab {orchestrator} has no live agent, so {} worker-loss notice(s) were withheld instead of being typed into its shell. Affected tasks: {tasks}. They are delivered once its agent is ready again; restart it with `codeterm agent restart --tab {orchestrator}`.",
                held.len()
            )),
            Some("warning"),
            None,
            Some(true),
            NotificationOrigin::local_tab(orchestrator.to_string()),
            PushIntent {
                level: Some(NotifyLevel::ActionNeeded),
                event_key: Some(format!("worker-lost-withheld:{orchestrator}")),
            },
            None,
        );
    }
}
