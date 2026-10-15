use super::ports::PlanEventSink;
use crate::ports::host::UiEvents;
use std::sync::Arc;

pub struct TauriPlanEventSink {
    ui: Arc<dyn UiEvents>,
}

impl TauriPlanEventSink {
    pub fn new(ui: Arc<dyn UiEvents>) -> Self {
        Self { ui }
    }
}

impl PlanEventSink for TauriPlanEventSink {
    fn emit_plan_changed(&self, plan_id: &str) {
        self.ui
            .emit("plan-changed", serde_json::json!({ "plan_id": plan_id }));
    }
}
