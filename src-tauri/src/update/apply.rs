use tauri_plugin_updater::UpdaterExt;
use url::Url;

use super::channel::{default_endpoints, Channel};
use super::shutdown;

pub struct ApplyOptions {
    pub channel: Channel,
    pub manifest_url: Option<String>,
}

pub async fn check_and_apply(
    app: &tauri::AppHandle,
    opts: ApplyOptions,
) -> Result<(), String> {
    let endpoint_str = opts
        .manifest_url
        .clone()
        .unwrap_or_else(|| default_endpoints(opts.channel)[0].clone());

    let endpoint = Url::parse(&endpoint_str)
        .map_err(|e| format!("invalid manifest URL '{}': {}", endpoint_str, e))?;

    log::info!(
        "[updater] check_and_apply channel={} url={}",
        opts.channel,
        endpoint
    );

    let mut builder = app.updater_builder();
    if opts.manifest_url.is_some() {
        builder = builder
            .endpoints(vec![endpoint])
            .map_err(|e| format!("failed to override updater endpoint: {}", e))?;
    }
    let updater = builder
        .build()
        .map_err(|e| format!("failed to build updater: {}", e))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("updater check failed: {}", e))?;

    let update = match update {
        Some(u) => u,
        None => {
            log::info!("[updater] no update available");
            return Ok(());
        }
    };

    log::info!("[updater] downloading and installing v{}", update.version);
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("download/install failed: {}", e))?;

    if let Err(e) = shutdown::graceful_shutdown(app).await {
        log::error!("[updater] graceful shutdown failed: {}", e);
    }

    log::info!("[updater] restarting application");
    app.restart();
}
