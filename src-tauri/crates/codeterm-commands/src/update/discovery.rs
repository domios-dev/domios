use codeterm_plugins::{AvailabilityItem, PluginCtx, UpdateKind};
use std::sync::Arc;

use codeterm_update::update_state::AvailabilitySnapshot;

static LAST: parking_lot::Mutex<Option<AvailabilitySnapshot>> = parking_lot::Mutex::new(None);

mod background;
static BACKGROUND: background::Refresh = background::Refresh::new();

pub fn cached_snapshot() -> AvailabilitySnapshot {
    let mut cached = LAST.lock().clone().unwrap_or_default();
    ensure_app(&mut cached);
    cached
}

pub fn refresh_in_background(
    ui: Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    state: codeterm_update::UpdateStateHandle,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) {
    let _ = BACKGROUND.start(async move {
        complete_background_check(&ui, check(&ui, &state, bundles, force)).await;
    });
}

async fn complete_background_check(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    check: impl std::future::Future<Output = Result<AvailabilitySnapshot, String>>,
) {
    match check.await {
        Ok(snapshot) => publish(ui, &snapshot),
        Err(error) => log::debug!("[update-discovery] background check failed: {error}"),
    }
}

pub async fn check(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    state: &codeterm_update::UpdateStateHandle,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) -> Result<AvailabilitySnapshot, String> {
    let generation = FLIGHT.generation();
    let mut cached = LAST.lock().clone().unwrap_or_default();
    ensure_app(&mut cached);
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
    let plugins = tokio::task::spawn_blocking(local_plugins)
        .await
        .map_err(|error| error.to_string())??;
    replace_plugins(&mut cached, plugins);
    if !force
        && (cache::fresh(cached.checked_at_ms, cache::now_ms())
            || (app_checked(&cached) && !codeterm_update::checker::automatic_check_due().await))
    {
        return Ok(cached);
    }
    FLIGHT
        .run(generation, || async {
            match refresh(ui, state, bundles, force).await {
                Ok(snapshot) => Ok(snapshot),
                Err(error) => {
                    record_failure(&mut cached, &error, cache::now_ms());
                    publish(ui, &cached);
                    Err(error)
                }
            }
        })
        .await
}

static FLIGHT: cache::SingleFlight<AvailabilitySnapshot> = cache::SingleFlight::new();
mod cache;
pub mod lifecycle;

async fn refresh(
    ui: &Arc<dyn codeterm_service_core::ports::host::UiEvents>,
    state: &codeterm_update::UpdateStateHandle,
    bundles: Option<Arc<codeterm_frontend_bundle::FrontendBundles>>,
    force: bool,
) -> Result<AvailabilitySnapshot, String> {
    let method = state.lock().install_method.clone();
    let app_result = codeterm_update::checker::check_with_diagnostic(
        &method,
        None,
        codeterm_update::check_throttle::CheckTrigger::from_force(force),
    )
    .await;
    if let Ok((info, _)) = &app_result {
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
        apply_app_check(&mut snapshot, app_result);
        match bundles
            .map(|bundles| frontend_offer(bundles.availability(), || bundles.local_availability()))
        {
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
    let plugins = tokio::task::spawn_blocking(local_plugins)
        .await
        .map_err(|error| error.to_string())??;
    replace_plugins(&mut result, plugins);
    result.checked_at_ms = Some(cache::now_ms());
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

/// A failed check stays in the log and the snapshot's diagnostics; the UI reads it as no update.
fn apply_app_check(
    snapshot: &mut AvailabilitySnapshot,
    result: Result<(codeterm_update::update_state::UpdateInfo, Option<String>), String>,
) {
    match result {
        Ok((info, error)) => {
            if let Some(path) = info.check_path {
                snapshot.served_by.insert("app".into(), path);
            }
            let mut item = info.availability;
            if let Some(error) = error {
                log::warn!(
                    "[update-discovery] app check failed, keeping the last manifest: {error}"
                );
                snapshot.errors.insert("app".into(), error);
                item = cached_app_availability(item);
            }
            snapshot.items.push(item);
        }
        Err(error) => {
            if error != codeterm_update::checker::CHECK_DEFERRED {
                log::warn!("[update-discovery] app check failed: {error}");
                snapshot.errors.insert("app".into(), error);
            }
            ensure_app(snapshot);
        }
    }
}

/// A frontend release that was never published is no offer, not a failed source.
fn frontend_offer(
    discovered: Result<AvailabilityItem, String>,
    local: impl FnOnce() -> AvailabilityItem,
) -> Result<AvailabilityItem, String> {
    match discovered {
        Err(error) if codeterm_plugins::update_source::is_missing_release_asset(&error) => {
            log::debug!("[update-discovery] no frontend release published: {error}");
            Ok(local())
        }
        other => other,
    }
}

fn app_checked(snapshot: &AvailabilitySnapshot) -> bool {
    snapshot
        .items
        .iter()
        .any(|item| item.kind == UpdateKind::App && item.reason != "not_checked")
}

fn record_failure(snapshot: &mut AvailabilitySnapshot, error: &str, now_ms: u64) {
    snapshot.checked_at_ms = Some(now_ms);
    snapshot.errors.insert("discovery".into(), error.into());
    ensure_app(snapshot);
}

fn ensure_app(snapshot: &mut AvailabilitySnapshot) {
    if !snapshot
        .items
        .iter()
        .any(|item| item.kind == UpdateKind::App)
    {
        snapshot.items.push(AvailabilityItem {
            id: "app".into(),
            kind: UpdateKind::App,
            installed: Some(codeterm_build_info::version().into()),
            latest: None,
            compatible: true,
            state: codeterm_plugins::AvailabilityState::UpToDate,
            reason: "not_checked".into(),
            channel: Some("official".into()),
        });
    }
}

fn cached_app_availability(mut item: AvailabilityItem) -> AvailabilityItem {
    item.reason = if item.state == codeterm_plugins::AvailabilityState::UpdateAvailable {
        "cached_manifest"
    } else {
        "cached_check"
    }
    .into();
    item
}

fn local_plugins() -> Result<Vec<AvailabilityItem>, String> {
    let listed = codeterm_plugins::run(
        PluginCtx {
            data_root: codeterm_service_core::paths::data_dir(),
            version: Some(codeterm_build_info::version().into()),
        },
        codeterm_plugins::PluginAction::List,
    )?;
    listed["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|plugin| {
            serde_json::from_value(plugin["update"]["availability"].clone())
                .map_err(|error| error.to_string())
        })
        .collect()
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
mod tests;

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
        let next = codeterm_frontend_bundle::selection::offer_availability(
            installed,
            Some(codeterm_frontend_bundle::selection::Offer {
                version: latest,
                compatible: item.compatible,
            }),
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
