pub use codeterm_supporter::*;
use serde_json::Value;
use std::sync::Arc;
pub mod registry;

pub fn runtime_with_notifications(svc: &super::ServiceContext) -> Arc<SupporterRuntime> {
    let runtime = runtime();
    let mux = svc.mux.clone();
    let ui = svc.ui.clone();
    let title_mux = svc.mux.clone();
    runtime.attach_tab_titles(Arc::new(move |tab| {
        title_mux.read().get_tab(tab).map(|tab| tab.title.clone())
    }));
    runtime.attach_visibility(
        Arc::new(move |tab| mux.read().get_tab(tab).and_then(|tab| tab.team_id.clone())),
        Arc::new(move |event| {
            ui.emit(
                "supporter-journal-appended",
                serde_json::to_value(event).unwrap_or_default(),
            )
        }),
    );
    let svc = svc.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| {
        svc.log_system_notice(
            alert.title,
            None,
            Some(
                if alert.level == codeterm_notification_core::contract::NotifyLevel::Failure {
                    "error"
                } else {
                    "warning"
                },
            ),
        );
    }));
    runtime
}

pub fn activity_status(runtime: &SupporterRuntime, cap: usize) -> Value {
    let mut status = runtime.status(cap);
    status["observation_scheduler"] = super::supporter_tab_state::scheduler::status();
    status
}
