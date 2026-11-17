use codeterm_analytics::{CanvasKind, CanvasOperation, Event, Worker};
use std::sync::Arc;

pub(super) fn element_observer(
    worker: Worker,
) -> codeterm_plan::service_plan::canvas_elements::ElementObserver {
    Arc::new(move |operation, kind| {
        if let (Some(operation), Some(kind)) = (
            CanvasOperation::from_catalog(operation),
            CanvasKind::from_catalog(kind),
        ) {
            let _ = worker.record_nowait(Event::CanvasElementSaved { operation, kind });
        }
    })
}

pub(super) fn plugin_observer(
    worker: Worker,
) -> crate::config::product_observer::PluginEnabledObserver {
    Arc::new(move |enabled| {
        let _ = worker.record_nowait(Event::PluginEnabledChanged { enabled });
    })
}
