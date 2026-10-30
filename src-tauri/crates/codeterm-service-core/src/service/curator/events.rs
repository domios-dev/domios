use super::*;
use codeterm_events::SystemEvent;
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct CuratorTabs(pub parking_lot::RwLock<std::collections::BTreeMap<String, String>>);

impl ServiceContext {
    #[cfg(any(test, feature = "test-support"))]
    pub fn reconcile_curator_for_test(&self, id: &str, now: u64) {
        self.reconcile_curator_id(id, now);
    }

    pub fn start_curator_events(&self) -> Result<(), String> {
        self.set_ext(CuratorTabs::default())
            .map_err(|_| "Curator runtime could not be started".to_string())?;
        let mut receiver = self.events.subscribe();
        let service = self.clone();
        self.rt.spawn(async move {
            let bootstrap = service.clone();
            let mut plans = match tokio::task::spawn_blocking(move || {
                bootstrap
                    .plan_store
                    .list_full()
                    .into_iter()
                    .map(|plan| plan.id)
                    .collect::<BTreeSet<_>>()
            })
            .await
            {
                Ok(plans) => plans,
                Err(error) => {
                    log::debug!("[curator] bootstrap: {error}");
                    return;
                }
            };
            let mut pending = plans.clone();
            loop {
                if !pending.is_empty() {
                    let worker = service.clone();
                    let batch = std::mem::take(&mut pending);
                    if let Err(error) = tokio::task::spawn_blocking(move || {
                        let Ok(now) = worker.curator_now() else {
                            return;
                        };
                        for id in batch {
                            worker.reconcile_curator_id(&id, now);
                        }
                    })
                    .await
                    {
                        log::debug!("[curator] event reconciliation: {error}");
                    }
                }
                match receiver.recv().await {
                    Ok(event) => match event.event {
                        SystemEvent::Generic { event_type, data }
                            if event_type == "plan-changed"
                                || event_type == "curator-reconcile" =>
                        {
                            if let Some(id) = data["plan_id"].as_str() {
                                plans.insert(id.to_string());
                                pending.insert(id.to_string());
                            }
                        }
                        SystemEvent::MuxStateChanged
                        | SystemEvent::TabClosed { .. }
                        | SystemEvent::AgentStarted { .. }
                        | SystemEvent::AgentIdle { .. }
                        | SystemEvent::ConfigChanged { .. }
                        | SystemEvent::ContributionsChanged { .. } => {
                            pending.extend(plans.iter().cloned())
                        }
                        SystemEvent::Generic { event_type, .. }
                            if event_type.starts_with("notification")
                                || event_type == "cron-run-finished" =>
                        {
                            pending.extend(plans.iter().cloned())
                        }
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        pending.extend(plans.iter().cloned())
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        Ok(())
    }

    pub(crate) fn reconcile_curator_id(&self, id: &str, now: u64) {
        let gate = self
            .extensions
            .get_or_init(CuratorReconciliations::default)
            .for_plan(id);
        let _guard = match gate.try_lock() {
            Some(guard) => guard,
            None => {
                #[cfg(test)]
                super::reconciliation::test_boundary::observe(self, id, false);
                gate.lock()
            }
        };
        let Some(cron) = self.extensions.try_get::<crate::CronState>() else {
            return;
        };
        let Some(plan) = self.plan_store.get(id) else {
            return;
        };
        #[cfg(test)]
        super::reconciliation::test_boundary::observe(self, id, true);
        if let Some(index) = self.extensions.try_get::<CuratorTabs>() {
            let mut tabs = index.0.write();
            tabs.retain(|_, plan| plan != id);
            if let Some(tab) = plan
                .control_loop
                .running
                .as_ref()
                .and_then(|run| run.tab_id.as_ref())
            {
                tabs.insert(tab.clone(), id.to_string());
            }
        }
        if let Some(tab) = plan
            .control_loop
            .running
            .as_ref()
            .and_then(|run| run.tab_id.as_deref())
        {
            let missing = { self.mux.read().get_tab(tab).is_none() };
            if missing {
                if let Err(error) = self.settle_closed_curator_tab(tab) {
                    log::error!("[curator] missing tab {tab}: {error}");
                }
                return;
            }
        }
        if let Err(error) = self.reconcile_curator_plan(cron, &plan, now) {
            log::debug!("[curator] reconcile {id}: {error}");
        }
    }
}
