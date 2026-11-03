use crate::ports::host::UiEvents;
use crate::service::ServiceContext;
use codeterm_plan::service_plan::change_feed::PlanChangeNotice;
use codeterm_plan::service_plan::PlanStore;

pub fn emit_committed_plan_change(ui: &dyn UiEvents, store: &PlanStore, plan_id: &str) {
    match store.take_committed_change(plan_id) {
        Some(PlanChangeNotice::Committed(change)) => {
            if let Ok(payload) = serde_json::to_value(change) {
                ui.emit("plan-changed", payload);
            }
        }
        Some(PlanChangeNotice::Resync) => ui.emit("plans-loaded", serde_json::json!({})),
        None => {}
    }
}

impl ServiceContext {
    pub fn emit_committed_plan_changed(&self, plan_id: &str) {
        emit_committed_plan_change(&*self.ui, &self.plan_store, plan_id);
    }
}
