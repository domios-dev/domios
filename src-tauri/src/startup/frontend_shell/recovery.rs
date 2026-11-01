use super::*;

fn recovery_document(
    current_url: Result<Url, String>,
    unavailable: &Url,
    host_recovery: bool,
    current: (u64, u64),
    expected: (u64, u64),
    attached: bool,
) -> Result<bool, String> {
    if current.0 == 0 || current != expected || !attached {
        return Err("Unavailable frontend generation was replaced".into());
    }
    if host_recovery {
        return Ok(true);
    }
    current_url.map(|url| same_navigation_document(&url, unavailable))
}

async fn reinitialize_client(app: codeterm_client_shell::app_runtime::AppHandle) {
    match crate::host_ports::native_client::retry_initialize_client(app.clone()).await {
        Ok(initialized) => {
            #[cfg(debug_assertions)]
            if initialized.native == crate::host_ports::native_client::NativeClientStatus::Attached
            {
                eprintln!(
                    "DOMIOS_DEV_ATTACHMENT {}",
                    serde_json::json!({ "protocol": initialized.ready.protocol,
                            "client_pid": std::process::id(), "server_pid": initialized.ready.process.pid,
                            "epoch": initialized.ready.epoch,
                            "api_address": initialized.ready.api_address.to_string() })
                );
            }
            if let Err(error) = recover_for_attachment(app.clone(), &initialized.ready).await {
                log::error!("[frontend-shell] attachment recovery failed: {error}");
                if let Some(window) = app
                    .get_webview_window("starting")
                    .or_else(|| app.get_webview_window("main"))
                {
                    show_start_failure(&window, &error);
                }
            }
        }
        Err(error) => {
            log::error!(
                "[frontend-shell] client retry refused (stated={}): {error}",
                error.stated_refusal()
            );
            if let Some(window) = app
                .get_webview_window("starting")
                .or_else(|| app.get_webview_window("main"))
            {
                show_start_failure(&window, error.message());
            }
        }
    }
}

pub(crate) fn retry(app: &codeterm_client_shell::app_runtime::AppHandle) {
    cancel_recovery(app);
    if let Some(starting) = app.get_webview_window("starting") {
        if let Ok(url) = starting_url() {
            let _ = starting.navigate(url);
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = reconnect::cancel_and_wait(&app).await {
            if let Some(window) = app
                .get_webview_window("starting")
                .or_else(|| app.get_webview_window("main"))
            {
                show_start_failure(&window, &error);
            }
            return;
        }
        let Some(attachment) = app
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
        else {
            reinitialize_client(app).await;
            return;
        };
        let Some(window) = app.get_webview_window("main") else {
            drop(attachment);
            reinitialize_client(app).await;
            return;
        };
        let initial_failed = {
            let supervisor = app.state::<ShellSupervisor>();
            supervisor
                .initial_document
                .failed(supervisor.active_view_generation.load(Ordering::Acquire))
        };
        if initial_failed {
            drop(attachment);
            reinitialize_client(app).await;
            return;
        }
        let identity = attachment.identity().clone();
        let server_epoch = attachment.epoch().to_owned();
        let origin = attachment.api_origin();
        drop(attachment);
        let target = frontend_url(&app, &origin).await;
        if !same_server_attachment(&app, &identity, &server_epoch) {
            return;
        }
        match target {
            Ok(url) => {
                if let Err(error) = navigate_frontend(&app, &window, url) {
                    log::error!("[frontend-shell] native frontend retry failed: {error}");
                    show_unavailable(&window);
                }
            }
            Err(error) => {
                // A dead server keeps its stale attachment; attach to or launch a live one.
                log::debug!(
                    "[frontend-shell] attached interface unreachable ({error}); reinitializing"
                );
                reinitialize_client(app).await;
            }
        }
    });
}

pub(crate) async fn recover_for_attachment(
    app: codeterm_client_shell::app_runtime::AppHandle,
    expected: &codeterm_types::server_bootstrap::ServerReady,
) -> Result<(), String> {
    ensure_server_attachment(&app, expected)?;
    let Some(window) = app.get_webview_window("main") else {
        crate::deep_links::register_deep_links(&app).map_err(|error| error.to_string())?;
        run_on_main_thread(app.clone(), |handle| {
            crate::startup::desktop_lifecycle::show_or_create(&handle)
                .map_err(|error| error.to_string())
        })
        .await?;
        ensure_server_attachment(&app, expected)?;
        start(app);
        return Ok(());
    };
    let unavailable = unavailable_url().map_err(|error| error.to_string())?;
    let can_reuse_window = {
        let supervisor = app.state::<ShellSupervisor>();
        let api = app.state::<ServerApiAttachmentSlot>().current();
        let reusable = supervisor
            .view_bootstrap
            .lock()
            .as_ref()
            .is_some_and(|bootstrap| {
                api.is_some_and(|api| {
                    bootstrap.origin == api.api_origin() && bootstrap.token == api.token()
                })
            });
        reusable
    };
    let initial_failed = {
        let supervisor = app.state::<ShellSupervisor>();
        supervisor
            .initial_document
            .failed(supervisor.active_view_generation.load(Ordering::Acquire))
    };
    if can_reuse_window && !initial_failed {
        ensure_server_attachment(&app, expected)?;
        let url = frontend_url(&app, &loopback_origin(expected.api_address)?).await?;
        ensure_server_attachment(&app, expected)?;
        navigate_frontend(&app, &window, url)?;
        return Ok(());
    }
    let (old_view, old_navigation, before, mut windows) = {
        let supervisor = app.state::<ShellSupervisor>();
        let before = *supervisor.windows.borrow();
        (
            supervisor.active_view_generation.load(Ordering::Acquire),
            supervisor.navigation.load(Ordering::Acquire),
            before,
            supervisor.windows.subscribe(),
        )
    };
    if !recovery_document(
        window.url().map_err(|error| error.to_string()),
        &unavailable,
        initial_failed || ON_RECOVERY_SURFACE.load(Ordering::Acquire),
        (old_view, old_navigation),
        (old_view, old_navigation),
        ensure_server_attachment(&app, expected).is_ok(),
    )? {
        start(app);
        return Ok(());
    }

    crate::deep_links::register_deep_links(&app).map_err(|error| error.to_string())?;
    if old_view == 0 || (!before.available && !initial_failed) {
        return Err("Unavailable frontend generation is no longer active".into());
    }
    let close_window = window.clone();
    let expected_attachment = expected.clone();
    run_on_main_thread(app.clone(), move |handle| {
        let state = handle.state::<ShellSupervisor>();
        if !recovery_document(
            close_window.url().map_err(|error| error.to_string()),
            &unavailable,
            state.initial_document.failed(old_view) || ON_RECOVERY_SURFACE.load(Ordering::Acquire),
            (
                state.active_view_generation.load(Ordering::Acquire),
                state.navigation.load(Ordering::Acquire),
            ),
            (old_view, old_navigation),
            ensure_server_attachment(&handle, &expected_attachment).is_ok(),
        )? {
            return Err("Unavailable frontend generation was replaced".into());
        }
        close_window.destroy().map_err(|error| error.to_string())
    })
    .await?;

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let epoch = *windows.borrow_and_update();
            if epoch.generation > before.generation && !epoch.available {
                return Ok::<(), String>(());
            }
            windows.changed().await.map_err(|error| error.to_string())?;
        }
    })
    .await
    .map_err(|_| "Unavailable frontend destruction deadline elapsed".to_string())??;
    ensure_server_attachment(&app, expected)?;

    run_on_main_thread(app.clone(), |handle| {
        crate::startup::desktop_lifecycle::show_or_create(&handle)
            .map_err(|error| error.to_string())
    })
    .await?;
    ensure_server_attachment(&app, expected)?;
    let new_view = app
        .state::<ShellSupervisor>()
        .active_view_generation
        .load(Ordering::Acquire);
    if new_view <= old_view {
        return Err("Recovered frontend did not advance its view generation".into());
    }
    start(app);
    Ok(())
}

#[cfg(test)]
mod tests;
