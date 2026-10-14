use super::ports::PlanEventSink;
use crate::app_runtime::AppHandle;

pub struct TauriPlanEventSink {
    app: AppHandle,
}

impl TauriPlanEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl PlanEventSink for TauriPlanEventSink {
    fn emit_plan_changed(&self, plan_id: &str) {
        crate::api::events::emit_everywhere(
            &self.app,
            "plan-changed",
            serde_json::json!({ "plan_id": plan_id }),
        );
    }
}
