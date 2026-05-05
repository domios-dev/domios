use tauri_plugin_updater::UpdaterExt;
use url::Url;

use super::channel::{default_endpoints, Channel};
use super::manifest;
use super::shutdown;
use super::{UpdatePhase, UpdateState};

#[derive(Clone)]
pub struct ApplyOptions {
    pub channel: Channel,
    pub manifest_url: Option<String>,
}

pub async fn check_and_apply(
    app: &tauri::AppHandle,
    opts: ApplyOptions,
    update_state: std::sync::Arc<parking_lot::Mutex<UpdateState>>,
) -> Result<bool, String> {
    let endpoint_str = opts
        .manifest_url
        .clone()
        .unwrap_or_else(|| default_endpoints(opts.channel)[0].clone());

    let endpoint = Url::parse(&endpoint_str)
        .map_err(|e| format!("invalid manifest URL '{}': {}", endpoint_str, e))?;

    let local_version = env!("CARGO_PKG_VERSION");
    let target_arch = std::env::consts::ARCH;
    let target_os = std::env::consts::OS;

    log::info!(
        "[updater] check_and_apply channel={} url={} local={} os={} arch={}",
        opts.channel,
        endpoint,
        local_version,
        target_os,
        target_arch,
    );

    let mut builder = app.updater_builder();
    if opts.manifest_url.is_some() {
        builder = builder
            .endpoints(vec![endpoint.clone()])
            .map_err(|e| format!("failed to override updater endpoint: {}", e))?;
    }
    let updater = builder
        .build()
        .map_err(|e| format!("failed to build updater: {}", e))?;

    let update = match updater.check().await {
        Ok(u) => u,
        Err(e) => {
            let msg = format!("updater.check() error: {}", e);
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
                "[updater] plugin returned None — running manual manifest probe (local={} os={} arch={})",
                local_version,
                target_os,
                target_arch,
            );
            diagnose_no_update(&endpoint_str, local_version).await;
            return Ok(false);
        }
    };

    update_state.lock().set_phase(UpdatePhase::Downloading);
    log::info!("[updater] downloading and installing v{}", update.version);
    if let Err(e) = update.download_and_install(|_, _| {}, || {}).await {
        let msg = format!("download/install failed: {}", e);
        log::error!("[updater] {}", msg);
        return Err(msg);
    }

    update_state.lock().set_phase(UpdatePhase::Applying);

    if let Err(e) = shutdown::graceful_shutdown(app).await {
        log::error!("[updater] graceful shutdown failed: {}", e);
    }

    log::info!("[updater] restarting application");
    app.restart();
}

async fn diagnose_no_update(endpoint: &str, local_version: &str) {
    match manifest::fetch(endpoint).await {
        Ok(m) => {
            let plat_key = manifest::current_platform_key().unwrap_or("<unknown>");
            let has_payload = m.platforms.contains_key(plat_key);
            log::warn!(
                "[updater] manual manifest probe: remote_version={} platform_key={} has_payload_for_platform={} platforms={:?}",
                m.version,
                plat_key,
                has_payload,
                m.platforms.keys().collect::<Vec<_>>(),
            );
            match manifest::diff(local_version, &m) {
                Ok(Some(v)) => log::error!(
                    "[updater] DIAGNOSTIC: manual compare says local {} < remote {} (UPDATE AVAILABLE) but plugin returned None — likely missing/mismatched platform key '{}' or signature issue",
                    local_version, v, plat_key,
                ),
                Ok(None) => log::info!(
                    "[updater] manual compare confirms local {} >= remote {} — already up to date",
                    local_version, m.version,
                ),
                Err(e) => log::error!("[updater] manual diff parse error: {}", e),
            }
        }
        Err(e) => log::warn!("[updater] manual manifest fetch failed: {}", e),
    }
}
