use tauri_plugin_updater::UpdaterExt;
use url::Url;

use codeterm_update::apply::{begin_apply, ApplyOptions, ApplyResult};
use codeterm_update::{manifest, InstallMethod, UpdatePhase, UpdateStateHandle};

use crate::app_runtime::AppHandle;
use crate::application_host::{ServerStop, ServerStopError};

pub(crate) async fn check_and_apply(
    app: &AppHandle,
    opts: ApplyOptions,
    update_state: UpdateStateHandle,
    server_stop: std::sync::Arc<dyn ServerStop>,
) -> Result<ApplyResult, String> {
    begin_apply(&update_state);
    if opts.install_method == InstallMethod::Deb {
        #[cfg(target_os = "linux")]
        {
            use tauri::Manager;
            let Some(package) =
                codeterm_update::debian::stage(opts.manifest_url.as_deref()).await?
            else {
                return Ok(ApplyResult::NoUpdate);
            };
            let target = server_stop.debian_quit_target()?;
            let resources = app.path().resource_dir().map_err(|e| e.to_string())?;
            server_stop
                .prepare_detached_update(&package.version)
                .await?;
            codeterm_update::debian::handoff::launch(package, target, resources).await?;
            update_state.lock().set_phase(UpdatePhase::Applying);
            return Ok(ApplyResult::Restarting);
        }
        #[cfg(not(target_os = "linux"))]
        return Err("Debian installation requires Linux".into());
    }

    check_and_apply_plugin(app, opts, update_state, server_stop).await
}

async fn check_and_apply_plugin(
    app: &AppHandle,
    opts: ApplyOptions,
    update_state: UpdateStateHandle,
    server_stop: std::sync::Arc<dyn ServerStop>,
) -> Result<ApplyResult, String> {
    let resolved_manifest = codeterm_update::checker::resolve_manifest(
        &opts.install_method,
        opts.channel,
        opts.manifest_url.as_deref(),
    )
    .await?;
    let endpoint_str = opts.manifest_url.clone().unwrap_or_else(|| {
        codeterm_update::channel::default_endpoints_for_install_method(
            opts.channel,
            &opts.install_method,
        )[0]
        .clone()
    });

    let endpoint = Url::parse(&endpoint_str)
        .map_err(|e| format!("invalid manifest URL '{}': {}", endpoint_str, e))?;

    let local_version = codeterm_build_info::version();
    let target_arch = std::env::consts::ARCH;
    let target_os = std::env::consts::OS;
    let platform_key = manifest::current_platform_key();

    log::info!(
        "[updater] check_and_apply channel={} url={} local={} os={} arch={} platform={:?}",
        opts.channel,
        endpoint,
        local_version,
        target_os,
        target_arch,
        platform_key,
    );

    let mut builder = app
        .updater_builder()
        .endpoints(vec![endpoint.clone()])
        .map_err(|e| format!("failed to set updater endpoint: {}", e))?;
    #[cfg(windows)]
    {
        let app_handle = app.clone();
        builder = builder.on_before_exit(move || {
            if let Err(error) = super::restart::schedule_cleanup(&app_handle).and_then(|receiver| {
                receiver
                    .recv()
                    .map_err(|error| format!("application cleanup was interrupted: {error}"))
            }) {
                log::error!("[updater] cleanup before installer exit failed: {error}");
            }
        });
    }
    if let Some(target) = platform_key {
        builder = builder.target(target);
    }
    let updater = builder
        .build()
        .map_err(|e| format!("failed to build updater: {}", e))?;

    let update = match prepare_update(&updater, &resolved_manifest) {
        Ok(u) => u,
        Err(e) => {
            let msg = format!("updater.restore_update() error: {}", e);
            log::error!("[updater] {}", msg);
            return Err(msg);
        }
    };

    let update = match update {
        Some(u) => {
            log::info!(
                "[updater] plugin reported update available: remote_version={} current_version={}",
                u.version,
                u.current_version,
            );
            u
        }
        None => {
            log::warn!(
                "[updater] plugin reported no applicable update (local={} os={} arch={})",
                local_version,
                target_os,
                target_arch,
            );

            return Ok(ApplyResult::NoUpdate);
        }
    };

    update_state.lock().set_phase(UpdatePhase::Downloading);
    log::info!("[updater] downloading and verifying v{}", update.version);
    let chunk_state = update_state.clone();
    let bytes = match update
        .download(
            move |chunk_len, content_length| {
                chunk_state
                    .lock()
                    .record_download_chunk(chunk_len as u64, content_length);
            },
            || {},
        )
        .await
    {
        Ok(bytes) => bytes,
        Err(e) => {
            let msg = format!("GET {} download/verify failed: {}", update.download_url, e);
            log::error!("[updater] {}", msg);
            return Err(msg);
        }
    };

    let verified_len = bytes.len();
    let sink = super::failure_report::AppFailureSink {
        update_state: update_state.clone(),
    };

    let target_version = update.version.clone();
    let server_identity = server_stop.install_identity()?;
    let server_stopped = std::sync::atomic::AtomicBool::new(false);
    let outcome = super::orchestrate::install_after_shutdown(
        bytes,
        // Defect 1: prove the update can land BEFORE any pane is destroyed.
        move || async move {
            tokio::task::spawn_blocking(move || {
                super::viability::plugin_install_viability(verified_len)
            })
            .await
            .map_err(|e| format!("viability probe task failed: {e}"))??;
            super::install_holders::preflight(server_identity).await
        },
        |holders| async {
            server_stop.prepare_update(&target_version).await?;
            update_state.lock().set_phase(UpdatePhase::Applying);
            stop_pair_server(server_stop.as_ref(), &server_stopped, holders).await?;
            Ok(Vec::new())
        },
        move |verified_bytes| async move {
            log::info!("[updater] starting verified installer after the server stopped");
            tokio::task::spawn_blocking(move || {
                codeterm_update::receipt::stage_app(&update.version);
                update
                    .install(verified_bytes)
                    .map_err(|e| format!("installer launch failed: {}", e))
            })
            .await
            .map_err(|e| format!("installer launch task failed: {e}"))?
        },
        &sink,
    )
    .await;
    if let Err(cause) = outcome {
        if server_stopped.load(std::sync::atomic::Ordering::SeqCst) {
            log::error!(
                "[updater] install failed after the server stopped; relaunching the installed pair: {cause}"
            );
            super::restart::restart_from_async(app).await?;
        }
        return Err(cause);
    }

    restart_after_plugin_install(app).await
}

/// A stop request that reached the server owes a relaunch even when its exit stays unconfirmed.
fn server_stop_owes_relaunch(result: &Result<(), ServerStopError>) -> bool {
    result.as_ref().map_or_else(|e| e.dispatched, |()| true)
}

/// Stops the attached server, then releases install holders; `stopped` records that a relaunch is owed.
async fn stop_pair_server(
    stop: &dyn ServerStop,
    stopped: &std::sync::atomic::AtomicBool,
    holders: super::install_holders::PreparedHolders,
) -> Result<(), String> {
    log::info!("[updater] stopping the attached server before installing the pair");
    let result = stop.stop_server().await;
    if server_stop_owes_relaunch(&result) {
        stopped.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    result.map_err(|e| {
        format!(
            "installer launch cancelled because the server did not stop: {}",
            e.reason
        )
    })?;
    holders.confirm_exited().await?;
    super::install_holders::release_before_installer().await
}

#[cfg(windows)]
async fn restart_after_plugin_install(_: &AppHandle) -> Result<ApplyResult, String> {
    // tauri-plugin-updater exits this process after launching the NSIS installer.
    Ok(ApplyResult::Restarting)
}

#[cfg(not(windows))]
async fn restart_after_plugin_install(app: &AppHandle) -> Result<ApplyResult, String> {
    super::restart::restart_from_async(app).await?;
    Ok(ApplyResult::Restarting)
}

fn prepare_update(
    updater: &tauri_plugin_updater::Updater,
    manifest: &manifest::Manifest,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let raw = serde_json::to_value(manifest).map_err(|error| error.to_string())?;
    updater
        .restore_update(raw)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod manifest_tests;

#[cfg(test)]
mod server_stop_tests;
