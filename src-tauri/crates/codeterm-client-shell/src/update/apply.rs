use tauri_plugin_updater::UpdaterExt;
use url::Url;

use codeterm_update::apply::{begin_apply, ApplyOptions, ApplyResult};
use codeterm_update::{manifest, InstallMethod, UpdatePhase, UpdateStateHandle};

use crate::app_runtime::AppHandle;
use crate::application_host::{ServerStop, ServerStopError};

mod staged_payload;
use staged_payload::{PayloadIdentity, StagedPayload};

static STAGED_PAYLOAD: StagedPayload = StagedPayload::new();

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
            server_stop.ensure_attached().await?;
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
            codeterm_update::debian::handoff::launch(
                package,
                target,
                resources,
                opts.initiating_tab_id,
            )
            .await?;
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
    // Restoring a resolved manifest skips the provider initialization in Updater::check.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let resolved_manifest = codeterm_update::checker::resolve_manifest(
        &opts.install_method,
        opts.channel,
        opts.manifest_url.as_deref(),
    )
    .await?;
    let endpoint_str = opts.manifest_url.clone().unwrap_or_else(|| {
        codeterm_update::channel::default_endpoint_for_install_method(
            opts.channel,
            &opts.install_method,
        )
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
        .map_err(|e| format!("failed to set updater endpoint: {}", e))?
        .configure_client(|client| {
            client
                .connect_timeout(std::time::Duration::from_secs(30))
                .read_timeout(std::time::Duration::from_secs(60))
                .timeout(std::time::Duration::from_secs(15 * 60))
        });
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

    let mut update = match update {
        Some(u) => {
            log::info!(
                "[updater] plugin reported update available: remote_version={} current_version={}",
                u.version,
                u.current_version,
            );
            u
        }
        None => {
            STAGED_PAYLOAD.clear();
            log::warn!(
                "[updater] plugin reported no applicable update (local={} os={} arch={})",
                local_version,
                target_os,
                target_arch,
            );

            return Ok(ApplyResult::NoUpdate);
        }
    };

    server_stop.ensure_attached().await?;
    let identity = PayloadIdentity {
        version: update.version.clone(),
        signature: update.signature.clone(),
    };
    let bytes = STAGED_PAYLOAD
        .get_or_download(
            identity,
            download_verified_update(&mut update, &update_state),
        )
        .await?;
    let verified_len = bytes.len();
    let sink = super::failure_report::AppFailureSink {
        update_state: update_state.clone(),
    };

    let target_version = update.version.clone();
    let evidence_target = target_version.clone();
    let evidence = tokio::task::spawn_blocking(move || {
        codeterm_update::installer_evidence::Evidence::begin(&evidence_target)
    })
    .await
    .map_err(|e| format!("installer evidence task failed: {e}"))??;
    let preflight_stop = server_stop.clone();
    let initiating_tab = opts.initiating_tab_id;
    let server_identity = server_stop.install_identity()?;
    #[cfg(windows)]
    if let Some(root) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    {
        let path = codeterm_update::native_handoff::installer_refusal::path(&root);
        tokio::task::spawn_blocking(move || match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("cannot clear prior installer refusal: {error}")),
        })
        .await
        .map_err(|error| format!("installer refusal cleanup task failed: {error}"))??;
    }
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
            super::install_holders::preflight(
                server_identity,
                preflight_stop.as_ref(),
                initiating_tab.as_deref(),
            )
            .await
        },
        |holders| async {
            server_stop.prepare_update(&target_version).await?;
            evidence.record_async("server_stop_requested", None).await?;
            update_state.lock().set_phase(UpdatePhase::Applying);
            stop_pair_server(server_stop.as_ref(), &server_stopped, holders).await?;
            evidence.record_async("server_stopped", None).await?;
            Ok(Vec::new())
        },
        |verified_bytes| async {
            STAGED_PAYLOAD.clear();
            log::info!("[updater] starting verified installer after the server stopped");
            let evidence = evidence.clone();
            let install_app = app.clone();
            tokio::task::spawn_blocking(move || {
                #[cfg(windows)]
                {
                    evidence.record("client_cleanup_started", None)?;
                    super::restart::schedule_cleanup(&install_app)?
                        .recv_timeout(std::time::Duration::from_secs(30))
                        .map_err(|e| {
                            format!(
                                "installer launch cancelled: client cleanup did not complete: {e}"
                            )
                        })?;
                    evidence.record("client_cleanup_completed", None)?;
                }
                #[cfg(not(windows))]
                let _ = install_app;
                codeterm_update::receipt::stage_app(&update.version);
                evidence.record("installer_call_started", None)?;
                let outcome = update
                    .install(verified_bytes.as_slice())
                    .map_err(|e| format!("installer launch failed: {}", e));
                evidence.record(
                    if outcome.is_ok() {
                        "installer_call_returned"
                    } else {
                        "installer_call_failed"
                    },
                    outcome.as_ref().err().map(String::as_str),
                )?;
                outcome
            })
            .await
            .map_err(|e| format!("installer launch task failed: {e}"))?
        },
        &sink,
    )
    .await;
    if let Err(cause) = outcome {
        if let Err(error) = evidence.record_async("failed", Some(&cause)).await {
            log::error!("[updater] failed to preserve installer failure: {error}");
        }
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
    let request = holders.shutdown_reservation_request();
    let reservation = stop.reserve_install_shutdown(request);
    let result = stop_reserved_server(stop, reservation).await;
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
    Ok(())
}

async fn stop_reserved_server(
    stop: &dyn ServerStop,
    reservation: impl std::future::Future<Output = Result<String, String>> + Send,
) -> Result<(), ServerStopError> {
    let reservation = reservation.await.map_err(|reason| ServerStopError {
        dispatched: false,
        reason,
    })?;
    let result = stop.stop_server().await;
    if result.as_ref().is_err_and(|error| !error.dispatched) {
        stop.release_install_shutdown(&reservation)
            .await
            .map_err(|reason| ServerStopError {
                dispatched: false,
                reason,
            })?;
    }
    result
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

struct DownloadFailure {
    message: String,
    outage: bool,
}

async fn download_verified_update(
    update: &mut tauri_plugin_updater::Update,
    update_state: &UpdateStateHandle,
) -> Result<Vec<u8>, String> {
    update_state.lock().set_phase(UpdatePhase::Downloading);
    log::info!("[updater] downloading and verifying v{}", update.version);
    log::info!(target: "native_updater", "download_started target_version={}", update.version);
    let direct_url = update.download_url.to_string();
    let bytes = match download_update(update, update_state).await {
        Ok(bytes) => Ok(bytes),
        Err(failure) if failure.outage => {
            // Only the address changes: the same Update verifies the same signature on these bytes.
            match codeterm_update::release_transport::official_asset_url(
                &direct_url,
                &failure.message,
                std::time::Duration::from_secs(60),
            )
            .await
            .and_then(|url| Url::parse(&url).map_err(|e| e.to_string()))
            {
                Ok(api_url) => {
                    log::warn!(
                        "[updater] downloading v{} through the GitHub Releases API",
                        update.version
                    );
                    update.download_url = api_url;
                    update_state.lock().set_phase(UpdatePhase::Downloading);
                    download_update(update, update_state).await.map_err(|api| {
                        codeterm_update::release_transport::both_failed(
                            &failure.message,
                            &api.message,
                        )
                    })
                }
                Err(both) => Err(both),
            }
        }
        Err(failure) => Err(failure.message),
    }
    .map_err(|cause| {
        let msg = format!("GET {direct_url} download/verify failed: {cause}");
        log::error!("[updater] {msg}");
        msg
    })?;

    log::info!(target: "native_updater", "download_verified bytes={}", bytes.len());
    Ok(bytes)
}

fn download_is_outage(error: &tauri_plugin_updater::Error) -> bool {
    match error {
        tauri_plugin_updater::Error::Reqwest(_) => true,
        tauri_plugin_updater::Error::Network(message) => {
            codeterm_update::release_transport::download_status_is_outage(message)
        }
        _ => false,
    }
}

async fn download_update(
    update: &tauri_plugin_updater::Update,
    update_state: &UpdateStateHandle,
) -> Result<Vec<u8>, DownloadFailure> {
    let chunk_state = update_state.clone();
    let mut downloaded = 0u64;
    let mut milestone = 0u64;
    update
        .download(
            move |chunk_len, content_length| {
                downloaded = downloaded.saturating_add(chunk_len as u64);
                let next = content_length.filter(|total| *total > 0)
                    .map(|total| downloaded.saturating_mul(4) / total)
                    .unwrap_or(downloaded / (16 * 1024 * 1024));
                if next > milestone {
                    milestone = next;
                    log::info!(target: "native_updater", "download_progress bytes={} total={:?}", downloaded, content_length);
                }
                chunk_state
                    .lock()
                    .record_download_chunk(chunk_len as u64, content_length);
            },
            || {},
        )
        .await
        .map_err(|error| DownloadFailure {
            outage: download_is_outage(&error),
            message: error.to_string(),
        })
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
