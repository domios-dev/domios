use codeterm_plugins::{AvailabilityItem, PluginCtx, UpdateKind};
use std::sync::Arc;

use codeterm_service_core::update_state::AvailabilitySnapshot;

static LAST: parking_lot::Mutex<Option<AvailabilitySnapshot>> = parking_lot::Mutex::new(None);

pub async fn check(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    state: &codeterm_service_core::UpdateStateHandle,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) -> Result<AvailabilitySnapshot, String> {
    let mut cached = LAST.lock().clone().unwrap_or_default();
    if let Some(bundles) = &bundles {
        let local = bundles.local_availability();
        let previous = cached
            .items
            .iter()
            .find(|item| item.kind == UpdateKind::Frontend);
        if local.state == codeterm_plugins::AvailabilityState::ReadyToApply
            || previous
                .is_none_or(|item| item.state == codeterm_plugins::AvailabilityState::ReadyToApply)
        {
            replace_frontend(&mut cached, local);
        }
    }
    if !cached.items.iter().any(|item| item.kind == UpdateKind::App) {
        cached.items.push(AvailabilityItem::release(
            "app",
            UpdateKind::App,
            codeterm_build_info::version(),
            codeterm_build_info::version(),
            true,
        ));
    }
    let ui = ui.clone();
    let state = state.clone();
    cache::serve(
        cached,
        force,
        move || async move { refresh(&ui, &state, bundles, force).await },
        |job| {
            if REFRESHING
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                )
                .is_ok()
            {
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = job.await {
                        log::debug!("[update] background discovery failed: {error}");
                    }
                    REFRESHING.store(false, std::sync::atomic::Ordering::Release);
                });
            }
        },
    )
    .await
}

static REFRESHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
mod cache;

async fn refresh(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    state: &codeterm_service_core::UpdateStateHandle,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) -> Result<AvailabilitySnapshot, String> {
    let method = state.lock().install_method.clone();
    let app_result = super::checker::check_for_update(&method).await;
    if let Ok(info) = &app_result {
        let mut state = state.lock();
        state.last_check = Some(info.clone());
        state.last_check_time = Some(std::time::Instant::now());
    }
    let ctx = PluginCtx {
        data_root: codeterm_service_core::paths::data_dir(),
        version: Some(codeterm_build_info::version().into()),
    };
    let stage_bundles = bundles.clone();
    let mut result = tokio::task::spawn_blocking(move || {
        let mut snapshot: AvailabilitySnapshot =
            serde_json::from_value(codeterm_plugins::update_inventory(ctx, force)?)
                .map_err(|e| e.to_string())?;
        match app_result {
            Ok(info) => snapshot.items.push(info.availability),
            Err(error) => {
                snapshot.errors.insert("app".into(), error);
            }
        }
        match bundles.map(|bundles| bundles.availability()) {
            Some(Ok(item)) => snapshot.items.push(item),
            Some(Err(error)) => {
                snapshot.errors.insert("frontend".into(), error);
            }
            None => {
                snapshot.errors.insert(
                    "frontend".into(),
                    "Frontend bundle store unavailable".into(),
                );
            }
        }
        snapshot.items.sort_by(|a, b| a.id.cmp(&b.id));
        Ok::<_, String>(snapshot)
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(bundles) = &stage_bundles {
        let local = bundles.local_availability();
        if local.state == codeterm_plugins::AvailabilityState::ReadyToApply {
            replace_frontend(&mut result, local);
        }
    }
    publish(ui, &result);
    if let Some(bundles) = stage_bundles {
        if let Some(item) = result
            .items
            .iter()
            .find(|item| item.kind == UpdateKind::Frontend)
        {
            let status = bundles.status();
            if codeterm_plugins::frontend_stage_needed(item, status["pending"]["version"].as_str())
            {
                codeterm_service_core::startup::frontend_bundles_check(bundles);
            }
        }
    }
    Ok(result)
}

pub fn publish(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    snapshot: &AvailabilitySnapshot,
) {
    let changed = {
        let mut last = LAST.lock();
        if last.as_ref() == Some(snapshot) {
            false
        } else {
            *last = Some(snapshot.clone());
            true
        }
    };
    if changed {
        ui.emit(
            "update-availability-changed",
            serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub fn replace_plugins(snapshot: &mut AvailabilitySnapshot, items: Vec<AvailabilityItem>) {
    snapshot
        .items
        .retain(|item| item.kind != UpdateKind::Plugin);
    snapshot.items.extend(items);
    snapshot.items.sort_by(|a, b| a.id.cmp(&b.id));
}

pub fn publish_plugins(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    items: Vec<AvailabilityItem>,
) {
    let next = {
        let mut last = LAST.lock();
        let mut snapshot = last.clone().unwrap_or_default();
        replace_plugins(&mut snapshot, items);
        if last.as_ref() == Some(&snapshot) {
            None
        } else {
            *last = Some(snapshot.clone());
            Some(snapshot)
        }
    };
    if let Some(snapshot) = next {
        ui.emit(
            "update-availability-changed",
            serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub async fn plugins_changed(
    ui: Arc<dyn codeterm_service_core::ports::host::UiEvents>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let inventory = codeterm_plugins::update_inventory(
            PluginCtx {
                data_root: codeterm_service_core::paths::data_dir(),
                version: Some(codeterm_build_info::version().into()),
            },
            false,
        )?;
        let snapshot: AvailabilitySnapshot =
            serde_json::from_value(inventory).map_err(|e| e.to_string())?;
        publish_plugins(&ui, snapshot.items);
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn publish_plugin_snapshot(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    snapshot: &serde_json::Value,
) {
    let items = snapshot["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|plugin| serde_json::from_value(plugin["update"]["availability"].clone()).ok())
        .collect();
    publish_plugins(ui, items);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn action_completion_replaces_installed_states_and_preserves_other_updates() {
        let mut snapshot = AvailabilitySnapshot::default();
        snapshot.items.push(AvailabilityItem::release(
            "app",
            UpdateKind::App,
            "1.0.0",
            "2.0.0",
            true,
        ));
        replace_plugins(
            &mut snapshot,
            vec![AvailabilityItem::release(
                "lark",
                UpdateKind::Plugin,
                "0.1.2",
                "0.1.3",
                true,
            )],
        );
        assert_eq!(
            snapshot.items[1].state,
            codeterm_plugins::AvailabilityState::UpdateAvailable
        );
        replace_plugins(
            &mut snapshot,
            vec![AvailabilityItem::release(
                "lark",
                UpdateKind::Plugin,
                "0.1.3",
                "0.1.3",
                true,
            )],
        );
        assert_eq!(
            snapshot.items[1].state,
            codeterm_plugins::AvailabilityState::UpToDate
        );
        assert_eq!(
            snapshot.items[0].state,
            codeterm_plugins::AvailabilityState::UpdateAvailable
        );
        replace_plugins(&mut snapshot, vec![]);
        assert_eq!(snapshot.items.len(), 1);
    }
}

pub fn frontend_installed(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    installed: &str,
) {
    let next = {
        let mut last = LAST.lock();
        let Some(snapshot) = last.as_mut() else {
            return;
        };
        let Some(item) = snapshot
            .items
            .iter_mut()
            .find(|item| item.kind == UpdateKind::Frontend)
        else {
            return;
        };
        let Some(latest) = item.latest.as_deref() else {
            return;
        };
        let next = AvailabilityItem::release(
            "frontend",
            UpdateKind::Frontend,
            installed,
            latest,
            item.compatible,
        );
        if *item == next {
            None
        } else {
            *item = next;
            Some(snapshot.clone())
        }
    };
    if let Some(snapshot) = next {
        ui.emit(
            "update-availability-changed",
            serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
        );
    }
}

pub fn replace_frontend(snapshot: &mut AvailabilitySnapshot, item: AvailabilityItem) {
    snapshot
        .items
        .retain(|item| item.kind != UpdateKind::Frontend);
    snapshot.items.push(item);
    snapshot.errors.remove("frontend");
    snapshot.items.sort_by(|a, b| a.id.cmp(&b.id));
}

pub fn publish_frontend(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    item: AvailabilityItem,
) {
    let next = {
        let mut last = LAST.lock();
        let mut snapshot = last.clone().unwrap_or_default();
        replace_frontend(&mut snapshot, item);
        if last.as_ref() == Some(&snapshot) {
            None
        } else {
            *last = Some(snapshot.clone());
            Some(snapshot)
        }
    };
    if let Some(snapshot) = next {
        ui.emit(
            "update-availability-changed",
            serde_json::to_value(snapshot).unwrap_or(serde_json::Value::Null),
        );
    }
}
