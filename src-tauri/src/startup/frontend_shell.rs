use codeterm_host_runtime::app_runtime::WebviewWindow;
#[cfg(debug_assertions)]
mod assets;

pub fn application_context(
    context: tauri::Context<codeterm_host_runtime::app_runtime::Runtime>,
) -> tauri::Context<codeterm_host_runtime::app_runtime::Runtime> {
    #[cfg(debug_assertions)]
    {
        let mut context = context;
        if context.config().build.dev_url.is_some() {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
            match assets::DevelopmentAssets::load(&root) {
                Ok(assets) => {
                    context.set_assets(Box::new(assets));
                    log::debug!("[frontend-shell] prepared development fallback assets installed");
                }
                Err(error) => {
                    log::error!("[frontend-shell] development fallback unavailable: {error}")
                }
            }
        }
        context
    }
    #[cfg(not(debug_assertions))]
    context
}

use codeterm_service_core::startup::{
    frontend_shell_fallback_origin as fallback_origin,
    frontend_shell_loopback_origin as loopback_origin,
};
use codeterm_service_core::startup::{RestApiControlState, RestApiPhase, RestApiStatus};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

mod model;
use model::{
    boot_failure_action, development_frontend, external_link, BootFailureAction, BootObservation,
};

#[derive(Default)]
struct ShellSupervisor {
    navigation: Arc<AtomicU64>,
    development_frontend: Option<Url>,
}

#[derive(Clone, Serialize)]
struct ShellBootstrap {
    origin: String,
    token: String,
    #[serde(rename = "frontendOrigin", skip_serializing_if = "Option::is_none")]
    frontend_origin: Option<String>,
}

fn ready_origin(
    status: &RestApiStatus,
    address: Option<SocketAddr>,
) -> Result<Option<String>, String> {
    if status.phase != RestApiPhase::Listening {
        return Ok(None);
    }
    let address = address.ok_or("Listening generation does not own a bound address")?;
    loopback_origin(address).map(Some)
}

fn bootstrap(
    config: &codeterm_service_core::config::schema::ApiConfig,
) -> Result<ShellBootstrap, String> {
    if !config.enabled {
        return Err("Daemon listener is disabled".into());
    }
    let token = config
        .token
        .clone()
        .filter(|token| !token.trim().is_empty())
        .ok_or("Desktop requires a daemon bearer token")?;
    Ok(ShellBootstrap {
        origin: loopback_origin(codeterm_service_core::config::canonical_api_bind_addr(
            config,
        )?)?,
        token,
        frontend_origin: None,
    })
}

fn bootstrap_script(bootstrap: &ShellBootstrap) -> Result<String, serde_json::Error> {
    let script = include_str!("frontend_shell_bootstrap.js").replace(
        "__CODETERM_SHELL_BOOTSTRAP__",
        &serde_json::to_string(bootstrap)?,
    );
    Ok(script.replace(
        "__CODETERM_SHELL_DEBUG__",
        if cfg!(debug_assertions) {
            "true"
        } else {
            "false"
        },
    ))
}

#[cfg(test)]
use codeterm_service_core::startup::frontend_shell_declared_frontend_origins as declared_frontend_origins;

fn fallback_url() -> Result<Url, url::ParseError> {
    Url::parse(&format!("{}/index.html", fallback_origin()))
}

fn unavailable_url() -> Result<Url, url::ParseError> {
    let html = include_str!("frontend_shell_unavailable.html");
    let encoded: String = url::form_urlencoded::byte_serialize(html.as_bytes()).collect();
    Url::parse(&format!("data:text/html;charset=utf-8,{encoded}"))
}

fn navigation_allowed(url: &Url, origin: Option<&str>, unavailable: &Url, fallback: &Url) -> bool {
    url == unavailable
        || url == fallback
        || origin.is_some_and(|origin| {
            url.origin().ascii_serialization() == origin
                && url.username().is_empty()
                && url.password().is_none()
        })
}

fn open_external_link(url: &Url, origin: Option<&str>) {
    let Some(url) = external_link(url, origin) else {
        return;
    };
    log::debug!(
        "[frontend-shell] opening external link: {}",
        navigation_identity(&url)
    );
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = codeterm_service_core::service::open_url_open_url(url.into()) {
            log::debug!("[frontend-shell] external link opener failed: {error}");
        }
    });
}

fn daemon_capability(origin: &str) -> Result<String, serde_json::Error> {
    let mut capability: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/default.json"))?;
    capability["identifier"] = serde_json::json!("daemon-shell");
    capability["local"] = serde_json::json!(false);
    capability["remote"] = serde_json::json!({ "urls": [format!("{origin}/*")] });
    serde_json::to_string(&capability)
}

pub fn create_window(
    app: &codeterm_host_runtime::app_runtime::App,
    config: &codeterm_service_core::config::schema::ApiConfig,
) -> Result<WebviewWindow, Box<dyn std::error::Error>> {
    let development_frontend =
        development_frontend(cfg!(debug_assertions), app.config().build.dev_url.as_ref())?;
    app.manage(ShellSupervisor {
        development_frontend: development_frontend.clone(),
        ..Default::default()
    });
    let mut bootstrap = bootstrap(config)
        .map_err(|error| {
            log::error!("[frontend-shell] {error}");
            error
        })
        .ok();
    if let Some(bootstrap) = bootstrap.as_mut() {
        bootstrap.frontend_origin = development_frontend
            .as_ref()
            .map(|url| url.origin().ascii_serialization());
    }
    let unavailable = unavailable_url()?;
    let fallback = fallback_url()?;
    let mut window_config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("Main window configuration is missing")?;
    window_config.url = WebviewUrl::CustomProtocol(unavailable.clone());
    let origin = bootstrap.as_ref().map(|bootstrap| {
        bootstrap
            .frontend_origin
            .clone()
            .unwrap_or_else(|| bootstrap.origin.clone())
    });
    log::debug!(
        "[frontend-shell] window creation: frontend_origin={origin:?}, daemon_origin={:?}",
        bootstrap.as_ref().map(|bootstrap| &bootstrap.origin)
    );
    let mut builder = WebviewWindowBuilder::from_config(app, &window_config)?;
    if let Some(bootstrap) = bootstrap {
        app.add_capability(daemon_capability(
            origin.as_deref().ok_or("Frontend origin is missing")?,
        )?)?;
        builder = builder.initialization_script(bootstrap_script(&bootstrap)?);
    }
    let new_window_origin = origin.clone();
    Ok(builder
        .on_new_window(move |url, _features| {
            open_external_link(&url, new_window_origin.as_deref());
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(|window, payload| {
            log::debug!(
                "[frontend-shell] page load {:?}: {}",
                payload.event(),
                navigation_identity(payload.url())
            );
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                observe_navigation(window, payload.url().clone());
            } else if cfg!(debug_assertions) {
                let _ = window.eval("window.__CODETERM_SHELL_DIAGNOSTIC__?.('page-finished')");
            }
        })
        .on_navigation(move |url| {
            let allowed = navigation_allowed(url, origin.as_deref(), &unavailable, &fallback);
            log::debug!(
                "[frontend-shell] navigation allowed={allowed}: {}",
                navigation_identity(url)
            );
            if !allowed {
                open_external_link(url, origin.as_deref());
            }
            allowed
        })
        .build()?)
}

fn navigation_identity(url: &Url) -> String {
    if url.scheme() == "data" {
        return "native recovery document".into();
    }
    let mut identity = url.clone();
    identity.set_query(None);
    identity.set_fragment(None);
    identity.to_string()
}

fn show_unavailable(window: &WebviewWindow) {
    match unavailable_url()
        .map_err(|error| error.to_string())
        .and_then(|url| window.navigate(url).map_err(|error| error.to_string()))
    {
        Ok(()) => log::debug!("[frontend-shell] native recovery surface selected"),
        Err(error) => log::error!("[frontend-shell] recovery surface failed: {error}"),
    }
}

fn observe_navigation(window: WebviewWindow, url: Url) {
    let app = window.app_handle();
    let navigation = app.state::<ShellSupervisor>().navigation.clone();
    let current_navigation = navigation.fetch_add(1, Ordering::AcqRel) + 1;
    if url.scheme() == "data" {
        log::debug!("[frontend-shell] boot observer skipped native recovery document");
        return;
    }
    let control = app.state::<RestApiControlState>().inner().clone();
    let listener = control.status();
    if listener.phase != RestApiPhase::Listening {
        log::debug!("[frontend-shell] boot observer skipped: listener={listener:?}");
        return;
    }
    let frontend_generation = codeterm_service_core::startup::observe_frontend_boot();
    let observation = BootObservation {
        navigation: current_navigation,
        listener_generation: listener.generation,
        frontend_generation,
        bundled_fallback: fallback_url().is_ok_and(|fallback| fallback == url),
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    log::debug!("[frontend-shell] boot observer armed: navigation={current_navigation}, listener={}, frontend={frontend_generation}, fallback={}", listener.generation, observation.bundled_fallback);
    tauri::async_runtime::spawn(async move {
        let ready = tokio::task::spawn_blocking(move || {
            super::wait_for_frontend_ready_until(frontend_generation, deadline)
        })
        .await
        .unwrap_or(false);
        if ready {
            log::debug!("[frontend-shell] root render acknowledged: navigation={current_navigation}, frontend={frontend_generation}");
            return;
        }
        let action = boot_failure_action(
            observation,
            navigation.load(Ordering::Acquire),
            &control.status(),
            codeterm_service_core::startup::frontend_readiness_generation(),
        );
        log::debug!("[frontend-shell] boot failure decision={action:?}, navigation={current_navigation}, frontend={frontend_generation}");
        match action {
            BootFailureAction::Ignore => {}
            BootFailureAction::BundledFallback => {
                log::warn!("[frontend-shell] root render was not acknowledged; selecting packaged fallback");
                match fallback_url() {
                    Ok(fallback) => {
                        if let Err(error) = window.navigate(fallback) {
                            log::error!(
                                "[frontend-shell] packaged fallback navigation failed: {error}"
                            );
                            show_unavailable(&window);
                        }
                    }
                    Err(error) => {
                        log::error!("[frontend-shell] packaged fallback URL failed: {error}");
                        show_unavailable(&window);
                    }
                }
            }
            BootFailureAction::Unavailable => {
                log::warn!("[frontend-shell] packaged root render was not acknowledged");
                show_unavailable(&window);
            }
        }
    });
}

async fn frontend_url(
    app: &codeterm_host_runtime::app_runtime::AppHandle,
    origin: &str,
) -> Result<Url, String> {
    if let Some(url) = &app.state::<ShellSupervisor>().development_frontend {
        log::debug!(
            "[frontend-shell] selecting configured development frontend: {}",
            navigation_identity(url)
        );
        return Ok(url.clone());
    }
    let bundles = app
        .try_state::<Arc<codeterm_frontend_bundle::FrontendBundles>>()
        .map(|bundles| bundles.inner().clone());
    let index =
        codeterm_service_core::startup::frontend_bundles_navigation_index(bundles.as_deref());
    let target = Url::parse(origin)
        .and_then(|origin| origin.join(&index))
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    let available = tokio::time::timeout(Duration::from_secs(10), async {
        let response = client.get(target.clone()).send().await?;
        let success = response.status().is_success();
        let bytes = response.bytes().await?;
        Ok::<_, reqwest::Error>(success && !bytes.is_empty())
    })
    .await
    .map_err(|_| "Frontend request exceeded its failure deadline".to_string())?
    .map_err(|error| error.to_string())?;
    if available {
        log::debug!(
            "[frontend-shell] daemon frontend acknowledged: {}",
            navigation_identity(&target)
        );
        Ok(target)
    } else {
        log::debug!(
            "[frontend-shell] daemon frontend index unavailable; selecting packaged fallback"
        );
        fallback_url().map_err(|error| error.to_string())
    }
}

pub fn start(app: codeterm_host_runtime::app_runtime::AppHandle, control: RestApiControlState) {
    let mut status = control.subscribe_status();
    tauri::async_runtime::spawn(async move {
        loop {
            let observed = status.borrow_and_update().clone();
            if let Some(window) = app.get_webview_window("main") {
                match ready_origin(&observed, control.bound_addr()) {
                    Ok(Some(origin)) => {
                        let target = tokio::select! {
                            target = frontend_url(&app, &origin) => target,
                            changed = status.changed() => {
                                if changed.is_err() { return; }
                                continue;
                            },
                        };
                        match target {
                            Ok(url) if control.status() == observed => {
                                log::debug!("[frontend-shell] navigating after listener acknowledgement: generation={}, url={}", observed.generation, navigation_identity(&url));
                                if let Err(error) = window.navigate(url) {
                                    log::error!("[frontend-shell] navigation failed: {error}");
                                    show_unavailable(&window);
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                log::error!("[frontend-shell] frontend unavailable: {error}");
                                show_unavailable(&window);
                            }
                        }
                    }
                    Ok(None) => show_unavailable(&window),
                    Err(error) => {
                        log::error!("[frontend-shell] {error}");
                        show_unavailable(&window);
                    }
                }
            }
            if status.changed().await.is_err() {
                return;
            }
        }
    });
}

pub fn retry(app: &codeterm_host_runtime::app_runtime::AppHandle) {
    let control = app.state::<RestApiControlState>().inner().clone();
    let observed = control.status();
    if observed.phase != RestApiPhase::Listening {
        if let Err(error) = control.port_released() {
            log::error!("[frontend-shell] native bind retry refused: {error}");
        }
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
        let target = match ready_origin(&observed, control.bound_addr()) {
            Ok(Some(origin)) => frontend_url(&app, &origin).await,
            _ => return,
        };
        if control.status() != observed {
            return;
        }
        match target {
            Ok(url) => {
                if let Err(error) = window.navigate(url) {
                    log::error!("[frontend-shell] native frontend retry failed: {error}");
                    show_unavailable(&window);
                }
            }
            Err(error) => {
                log::error!("[frontend-shell] native frontend retry refused: {error}");
                show_unavailable(&window);
            }
        }
    });
}

#[cfg(test)]
mod tests;
