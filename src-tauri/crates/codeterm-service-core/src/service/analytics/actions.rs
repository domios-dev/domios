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

pub(super) fn canvas_file_events(
    outcome: &Result<codeterm_local_preview::html_preview::CanvasWrite, String>,
    before: &str,
    after: &str,
) -> Vec<Event> {
    use serde_json::Value;
    use std::collections::BTreeMap;
    if !matches!(
        outcome,
        Ok(codeterm_local_preview::html_preview::CanvasWrite::Written)
    ) || before.len() as u64 > codeterm_local_preview::html_preview::CANVAS_FILE_LIMIT
        || after.len() as u64 > codeterm_local_preview::html_preview::CANVAS_FILE_LIMIT
    {
        return Vec::new();
    }
    let (Ok(before), Ok(after)) = (
        serde_json::from_str::<Value>(before),
        serde_json::from_str::<Value>(after),
    ) else {
        return Vec::new();
    };
    fn items(document: &Value) -> Option<BTreeMap<String, &Value>> {
        let items = document.get("items")?.as_array()?;
        if items.len() > 64 {
            return None;
        }
        Some(
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    (
                        item.get("id")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("item-{}", index + 1)),
                        item,
                    )
                })
                .collect(),
        )
    }
    let (Some(before), Some(after)) = (items(&before), items(&after)) else {
        return Vec::new();
    };
    let event = |operation, item: &Value| Event::CanvasElementSaved {
        operation,
        kind: item
            .get("kind")
            .and_then(Value::as_str)
            .and_then(CanvasKind::from_catalog)
            .unwrap_or(CanvasKind::Other),
    };
    let mut events = Vec::new();
    for (id, item) in &after {
        match before.get(id) {
            None => events.push(event(CanvasOperation::Create, item)),
            Some(previous)
                if ["kind", "src", "html", "markdown", "title"]
                    .iter()
                    .any(|field| previous.get(*field) != item.get(*field)) =>
            {
                events.push(event(CanvasOperation::Update, item))
            }
            Some(_) => {}
        }
    }
    for (id, item) in &before {
        if !after.contains_key(id) {
            events.push(event(CanvasOperation::Delete, item));
        }
    }
    events
}
