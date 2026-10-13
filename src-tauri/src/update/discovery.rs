use codeterm_plugins::{AvailabilityItem, PluginCtx, UpdateKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AvailabilitySnapshot {
    pub items: Vec<AvailabilityItem>,
    pub errors: BTreeMap<String, String>,
}

static LAST: parking_lot::Mutex<Option<AvailabilitySnapshot>> = parking_lot::Mutex::new(None);

pub async fn check(
    app: &tauri::AppHandle,
    state: &crate::UpdateStateHandle,
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
        data_root: crate::paths::data_dir(),
        version: Some(env!("CARGO_PKG_VERSION").into()),
    };
    let stage_bundles = bundles.clone();
    let result = tokio::task::spawn_blocking(move || {
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
    publish(app, &result);
    if let Some(bundles) = stage_bundles {
        if let Some(item) = result
            .items
            .iter()
            .find(|item| item.kind == UpdateKind::Frontend)
        {
            let status = bundles.status();
            if codeterm_plugins::frontend_stage_needed(item, status["pending"]["version"].as_str())
            {
                crate::startup::frontend_bundles::check(bundles);
            }
        }
    }
    Ok(result)
}

pub fn publish(app: &tauri::AppHandle, snapshot: &AvailabilitySnapshot) {
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
        crate::api::events::emit_everywhere(app, "update-availability-changed", snapshot);
    }
}

pub fn replace_plugins(snapshot: &mut AvailabilitySnapshot, items: Vec<AvailabilityItem>) {
    snapshot
        .items
        .retain(|item| item.kind != UpdateKind::Plugin);
    snapshot.items.extend(items);
    snapshot.items.sort_by(|a, b| a.id.cmp(&b.id));
}

pub fn publish_plugins(app: &tauri::AppHandle, items: Vec<AvailabilityItem>) {
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
        crate::api::events::emit_everywhere(app, "update-availability-changed", snapshot);
    }
}

pub async fn plugins_changed(app: tauri::AppHandle) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let inventory = codeterm_plugins::update_inventory(
            PluginCtx {
                data_root: crate::paths::data_dir(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            },
            false,
        )?;
        let snapshot: AvailabilitySnapshot =
            serde_json::from_value(inventory).map_err(|e| e.to_string())?;
        publish_plugins(&app, snapshot.items);
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn publish_plugin_snapshot(app: &tauri::AppHandle, snapshot: &serde_json::Value) {
    let items = snapshot["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|plugin| serde_json::from_value(plugin["update"]["availability"].clone()).ok())
        .collect();
    publish_plugins(app, items);
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

pub fn frontend_installed(app: &tauri::AppHandle, installed: &str) {
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
        crate::api::events::emit_everywhere(app, "update-availability-changed", snapshot);
    }
}
