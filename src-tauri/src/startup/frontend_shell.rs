use super::{RestApiControlState, RestApiPhase, RestApiStatus};
use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::Url;

#[derive(Clone, Serialize)]
struct ShellBootstrap {
    origin: String,
    token: String,
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
    let bootstrap = bootstrap(config)
        .map_err(|error| {
            log::error!("[frontend-shell] {error}");
            error
        })
        .ok();
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
    let origin = bootstrap.as_ref().map(|bootstrap| bootstrap.origin.clone());
    let mut builder = WebviewWindowBuilder::from_config(app, &window_config)?;
    if let Some(bootstrap) = bootstrap {
        app.add_capability(daemon_capability(&bootstrap.origin)?)?;
        builder = builder.initialization_script(bootstrap_script(&bootstrap)?);
    }
    Ok(builder
        .on_navigation(move |url| {
            navigation_allowed(url, origin.as_deref(), &unavailable, &fallback)
        })
        .build()?)
}

async fn frontend_url(app: &tauri::AppHandle, origin: &str) -> Result<Url, String> {
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
                    Ok(Some(origin)) => match frontend_url(&app, &origin).await {
                        Ok(url) if control.status() == observed => {
                            if let Err(error) = window.navigate(url) {
                                log::error!("[frontend-shell] navigation failed: {error}");
                            }
                        }
                        Ok(_) => {}
                        Err(error) => log::error!("[frontend-shell] frontend unavailable: {error}"),
                    },
                    Ok(None) => {}
                    Err(error) => log::error!("[frontend-shell] {error}"),
                }
            }
            if status.changed().await.is_err() {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests;
