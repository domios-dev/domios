use super::{RestApiControlState, RestApiPhase, RestApiStatus};
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::Url;

mod model;
use model::{boot_failure_action, development_frontend, BootFailureAction, BootObservation};

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

fn loopback_origin(address: SocketAddr) -> Result<String, String> {
    let address = match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), address.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(Ipv6Addr::LOCALHOST.into(), address.port())
        }
        ip if ip.is_loopback() => address,
        _ => return Err("Desktop requires a loopback daemon listener".into()),
    };
    Ok(format!("http://{address}"))
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

fn bootstrap(config: &crate::config::schema::ApiConfig) -> Result<ShellBootstrap, String> {
    if !config.enabled {
        return Err("Daemon listener is disabled".into());
    }
    let token = config
        .token
        .clone()
        .filter(|token| !token.trim().is_empty())
        .ok_or("Desktop requires a daemon bearer token")?;
    Ok(ShellBootstrap {
        origin: loopback_origin(crate::config::canonical_api_bind_addr(config)?)?,
        token,
        frontend_origin: None,
    })
}

fn bootstrap_script(bootstrap: &ShellBootstrap) -> Result<String, serde_json::Error> {
    Ok(include_str!("frontend_shell_bootstrap.js").replace(
        "__CODETERM_SHELL_BOOTSTRAP__",
        &serde_json::to_string(bootstrap)?,
    ))
}

fn fallback_url() -> Result<Url, url::ParseError> {
    let origin = if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost"
    } else {
        "tauri://localhost"
    };
    Url::parse(&format!("{origin}/index.html"))
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

fn daemon_capability(origin: &str) -> Result<String, serde_json::Error> {
    let mut capability: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/default.json"))?;
    capability["identifier"] = serde_json::json!("daemon-shell");
    capability["local"] = serde_json::json!(false);
    capability["remote"] = serde_json::json!({ "urls": [format!("{origin}/*")] });
    serde_json::to_string(&capability)
}

pub(crate) fn create_window(
    app: &tauri::App,
    config: &crate::config::schema::ApiConfig,
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
    let mut builder = WebviewWindowBuilder::from_config(app, &window_config)?;
    if let Some(bootstrap) = bootstrap {
        app.add_capability(daemon_capability(
            origin.as_deref().ok_or("Frontend origin is missing")?,
        )?)?;
        builder = builder.initialization_script(bootstrap_script(&bootstrap)?);
    }
    Ok(builder
        .on_page_load(|window, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                observe_navigation(window, payload.url().clone());
            }
        })
        .on_navigation(move |url| {
            navigation_allowed(url, origin.as_deref(), &unavailable, &fallback)
        })
        .build()?)
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
        return;
    }
    let control = app.state::<RestApiControlState>().inner().clone();
    let listener = control.status();
    if listener.phase != RestApiPhase::Listening {
        return;
    }
    let frontend_generation = super::observe_frontend_boot();
    let observation = BootObservation {
        navigation: current_navigation,
        listener_generation: listener.generation,
        frontend_generation,
        bundled_fallback: fallback_url().is_ok_and(|fallback| fallback == url),
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    tauri::async_runtime::spawn(async move {
        let ready = tokio::task::spawn_blocking(move || {
            super::wait_for_frontend_ready_until(frontend_generation, deadline)
        })
        .await
        .unwrap_or(false);
        if ready {
            return;
        }
        match boot_failure_action(
            observation,
            navigation.load(Ordering::Acquire),
            &control.status(),
            super::frontend_readiness_generation(),
        ) {
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

async fn frontend_url(app: &tauri::AppHandle, origin: &str) -> Result<Url, String> {
    if let Some(url) = &app.state::<ShellSupervisor>().development_frontend {
        return Ok(url.clone());
    }
    let bundles = app
        .try_state::<Arc<crate::frontend_bundle::FrontendBundles>>()
        .map(|bundles| bundles.inner().clone());
    let index = super::frontend_bundles::navigation_index(bundles.as_deref());
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
        Ok(target)
    } else {
        fallback_url().map_err(|error| error.to_string())
    }
}

pub(crate) fn start(app: tauri::AppHandle, control: RestApiControlState) {
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

pub(crate) fn retry(app: &tauri::AppHandle) {
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
