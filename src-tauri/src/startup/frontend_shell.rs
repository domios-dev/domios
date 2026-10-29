use codeterm_client_shell::app_runtime::WebviewWindow;
#[cfg(debug_assertions)]
mod assets;
mod readiness;

pub fn application_context(
    context: tauri::Context<codeterm_client_shell::app_runtime::Runtime>,
) -> tauri::Context<codeterm_client_shell::app_runtime::Runtime> {
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

#[cfg(test)]
use crate::host_ports::webview_diagnostics::CLIENT_VIEW_QUERY;
use crate::host_ports::ClientViewGeneration;
use crate::host_ports::{
    webview_diagnostics::{root_generation_from_url, with_root_generation},
    ClientRootRenderLatch, ClientRootRenderWait, ServerApiAttachment, ServerApiAttachmentSlot,
};
use codeterm_frontend_shell::{
    brand_fallback_page, development_frontend, fallback_origin, loopback_origin, BrandFallback,
};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use url::Url;

fn same_origin(url: &Url, origin: Option<&str>) -> bool {
    origin
        .and_then(|origin| Url::parse(origin).ok())
        .is_some_and(|origin| url.origin() == origin.origin())
}

fn local_api_origin(url: &Url, api_origin: Option<&str>) -> bool {
    fn loopback(url: &Url) -> bool {
        match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            Some(url::Host::Domain(host)) => host == "localhost",
            None => false,
        }
    }

    api_origin
        .and_then(|origin| Url::parse(origin).ok())
        .is_some_and(|origin| {
            matches!(url.scheme(), "http" | "https")
                && url.scheme() == origin.scheme()
                && url.port_or_known_default() == origin.port_or_known_default()
                && loopback(url)
                && loopback(&origin)
        })
}

fn current_api_origin(app: &codeterm_client_shell::app_runtime::AppHandle) -> Option<String> {
    app.try_state::<ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current())
        .map(|attachment| attachment.api_origin())
        .or_else(|| {
            app.state::<ShellSupervisor>()
                .view_bootstrap
                .lock()
                .as_ref()
                .map(|bootstrap| bootstrap.origin.clone())
        })
}

fn external_link(url: &Url, origin: Option<&str>, api_origin: Option<&str>) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https" | "mailto")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str() == Some("tauri.localhost")
        || same_origin(url, origin)
        || local_api_origin(url, api_origin)
    {
        return None;
    }
    Some(url.clone())
}

struct ShellSupervisor {
    navigation: Arc<AtomicU64>,
    view_generation: AtomicU64,
    active_view_generation: AtomicU64,
    observer_started: std::sync::atomic::AtomicBool,
    root_render: ClientRootRenderLatch,
    development_frontend: Option<Url>,
    windows: tokio::sync::watch::Sender<WindowEpoch>,
    capability_origins: parking_lot::Mutex<HashSet<String>>,
    view_bootstrap: parking_lot::Mutex<Option<ShellBootstrap>>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct WindowEpoch {
    generation: u64,
    available: bool,
}

#[derive(Clone, Serialize)]
struct ShellBootstrap {
    origin: String,
    token: String,
    #[serde(rename = "frontendOrigin", skip_serializing_if = "Option::is_none")]
    frontend_origin: Option<String>,
}

fn bootstrap_attachment(attachment: &ServerApiAttachment) -> Result<ShellBootstrap, String> {
    if attachment.token().trim().is_empty() {
        return Err("Server attachment has no API credential".into());
    }
    Ok(ShellBootstrap {
        origin: attachment.api_origin(),
        token: attachment.token().to_owned(),
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

fn fallback_url() -> Result<Url, url::ParseError> {
    Url::parse(&format!("{}/index.html", fallback_origin()))
}

/// Navigating to an absent packaged index paints the webview's raw "asset not found" text.
fn packaged_fallback(app: &codeterm_client_shell::app_runtime::AppHandle) -> Result<Url, String> {
    if app.asset_resolver().get("index.html".into()).is_none() {
        return Err("packaged fallback index is absent".into());
    }
    fallback_url().map_err(|error| error.to_string())
}

fn unavailable_url() -> Result<Url, url::ParseError> {
    brand_page_url(BrandFallback::Reconnecting)
}

fn starting_url() -> Result<Url, url::ParseError> {
    brand_page_url(BrandFallback::Starting)
}

const START_FAILURE_DETAIL_CHARS: usize = 600;

fn start_failure_url(detail: Option<&str>) -> Result<Url, url::ParseError> {
    let mut url = brand_page_url(BrandFallback::StartFailed)?;
    if let Some(detail) = detail {
        let detail = detail
            .chars()
            .take(START_FAILURE_DETAIL_CHARS)
            .collect::<String>();
        url.set_fragment(Some(&component_encode(&detail)));
    }
    Ok(url)
}

fn brand_page_url(kind: BrandFallback) -> Result<Url, url::ParseError> {
    let html = brand_fallback_page(kind);
    Url::parse(&format!(
        "data:text/html;charset=utf-8,{}",
        component_encode(&html)
    ))
}

/// Form encoding turns spaces into '+', which a data: URL and decodeURIComponent keep literally.
fn component_encode(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

fn navigation_allowed(
    url: &Url,
    origin: Option<&str>,
    api_origin: Option<&str>,
    unavailable: &Url,
    fallback: &Url,
) -> bool {
    same_navigation_document(url, unavailable)
        || starting_url().is_ok_and(|starting| same_navigation_document(url, &starting))
        || start_failure_url(None).is_ok_and(|failed| same_navigation_document(url, &failed))
        || same_navigation_document(url, fallback)
        || ((same_origin(url, origin) || local_api_origin(url, api_origin))
            && url.username().is_empty()
            && url.password().is_none())
}

fn open_external_link(url: &Url, origin: Option<&str>, api_origin: Option<&str>) {
    let Some(url) = external_link(url, origin, api_origin) else {
        return;
    };
    log::debug!(
        "[frontend-shell] opening external link: {}",
        navigation_identity(&url)
    );
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = codeterm_util::open_url::open_url(url.into()) {
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

pub fn initialize(
    app: &codeterm_client_shell::app_runtime::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    if app.try_state::<crate::ClientStartupOptions>().is_none()
        && !app.manage(crate::ClientStartupOptions::default())
    {
        return Err("Client startup options could not be installed".into());
    }
    let development_frontend =
        development_frontend(cfg!(debug_assertions), app.config().build.dev_url.as_ref())?;
    let root_render = ClientRootRenderLatch::default();
    if !app.manage(root_render.clone()) {
        return Err("Client root-render latch could not be installed".into());
    }
    app.manage(ShellSupervisor {
        development_frontend,
        navigation: Arc::new(AtomicU64::new(0)),
        view_generation: AtomicU64::new(0),
        active_view_generation: AtomicU64::new(0),
        observer_started: std::sync::atomic::AtomicBool::new(false),
        root_render,
        windows: tokio::sync::watch::channel(WindowEpoch::default()).0,
        capability_origins: Default::default(),
        view_bootstrap: Default::default(),
    });
    Ok(())
}

pub(crate) fn native_view_origins(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    ready: &codeterm_types::server_bootstrap::ServerReady,
) -> HashSet<String> {
    let mut origins = HashSet::from([fallback_origin().to_string()]);
    if let Ok(origin) = loopback_origin(ready.api_address) {
        origins.insert(origin);
    }
    if let Ok(Some(frontend)) =
        development_frontend(cfg!(debug_assertions), app.config().build.dev_url.as_ref())
    {
        origins.insert(frontend.origin().ascii_serialization());
    }
    origins
}

pub fn window_created(app: &codeterm_client_shell::app_runtime::AppHandle) {
    app.state::<ShellSupervisor>().windows.send_modify(|epoch| {
        epoch.generation += 1;
        epoch.available = true;
    });
}

pub fn window_destroying(app: &codeterm_client_shell::app_runtime::AppHandle) {
    if let Some(client) = app.try_state::<crate::host_ports::native_client::NativeHostClientSlot>()
    {
        client.release_view();
    }
    let supervisor = app.state::<ShellSupervisor>();
    let _ = next_generation(&supervisor.navigation);
    supervisor
        .active_view_generation
        .store(0, Ordering::Release);
    supervisor.root_render.retire();
    supervisor.windows.send_if_modified(|epoch| {
        if !epoch.available {
            return false;
        }
        epoch.generation += 1;
        epoch.available = false;
        true
    });
}

pub fn create_window(
    app: &codeterm_client_shell::app_runtime::AppHandle,
) -> Result<WebviewWindow, Box<dyn std::error::Error>> {
    let supervisor = app.state::<ShellSupervisor>();
    let development_frontend = supervisor.development_frontend.clone();
    let view_generation = next_generation(&supervisor.view_generation)
        .ok_or("Native view generation space is exhausted")?;
    let attachment = app
        .try_state::<ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current());
    let mut bootstrap =
        attachment
            .as_ref()
            .and_then(|attachment| match bootstrap_attachment(attachment) {
                Ok(bootstrap) => Some(bootstrap),
                Err(error) => {
                    log::error!("[frontend-shell] {error}");
                    None
                }
            });
    if attachment.is_none() {
        log::error!("[frontend-shell] no authenticated server attachment is available");
    }
    if let Some(bootstrap) = bootstrap.as_mut() {
        bootstrap.frontend_origin = development_frontend
            .as_ref()
            .map(|url| url.origin().ascii_serialization());
    }
    *supervisor.view_bootstrap.lock() = bootstrap.clone();
    let unavailable = unavailable_url()?;
    let fallback = fallback_url()?;
    let mut window_config = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("Main window configuration is missing")?;
    // Starting is not the retrying recovery page: a failed first attempt must still switch to it.
    window_config.url = WebviewUrl::CustomProtocol(starting_url()?);
    ON_RECOVERY_SURFACE.store(false, Ordering::Release);
    // Revealed on the first painted interface (or the recovery page), so startup never shows an empty frame.
    window_config.visible = false;
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
    let shell_app = app.clone();
    let mut builder = WebviewWindowBuilder::from_config(app, &window_config)?;
    #[cfg(windows)]
    if let crate::LocalWebviewDiagnosticsMode::RequestedPort(port) = app
        .state::<crate::ClientStartupOptions>()
        .local_webview_diagnostics
    {
        let available = cfg!(debug_assertions)
            && crate::host_ports::webview_diagnostics::port_is_available(port);
        if let Some(slot) =
            app.try_state::<crate::host_ports::native_client::NativeHostClientSlot>()
        {
            slot.set_diagnostics_port_preflight(port, available);
        }
        if available {
            builder = builder.additional_browser_args(&format!(
                "--remote-debugging-address=127.0.0.1 --remote-debugging-port={port}"
            ));
        }
    }
    if let Some(bootstrap) = bootstrap {
        let origin = origin.as_deref().ok_or("Frontend origin is missing")?;
        let supervisor = app.state::<ShellSupervisor>();
        let mut installed = supervisor.capability_origins.lock();
        if !installed.contains(origin) {
            app.add_capability(daemon_capability(origin)?)?;
            installed.insert(origin.to_string());
        }
        builder = builder.initialization_script(bootstrap_script(&bootstrap)?);
    }
    let new_window_origin = origin.clone();
    let new_window_app = app.clone();
    let page_origin = origin.clone();
    let page_fallback = fallback.clone();
    let page_unavailable = unavailable.clone();
    supervisor
        .active_view_generation
        .store(view_generation, Ordering::Release);
    match builder
        .on_new_window(move |url, _features| {
            let api_origin = current_api_origin(&new_window_app);
            open_external_link(&url, new_window_origin.as_deref(), api_origin.as_deref());
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |window, payload| {
            log::debug!(
                "[frontend-shell] page load {:?}: {}",
                payload.event(),
                navigation_identity(payload.url())
            );
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                observe_navigation(
                    window,
                    payload.url().clone(),
                    view_generation,
                    page_origin.as_deref(),
                    &page_unavailable,
                    &page_fallback,
                );
            } else if cfg!(debug_assertions) {
                let _ = window.eval("window.__CODETERM_SHELL_DIAGNOSTIC__?.('page-finished')");
            }
        })
        .on_navigation(move |url| {
            if url.scheme() == SHELL_ACTION_SCHEME {
                run_shell_action(&shell_app, url);
                return false;
            }
            let api_origin = current_api_origin(&shell_app);
            let allowed = navigation_allowed(
                url,
                origin.as_deref(),
                api_origin.as_deref(),
                &unavailable,
                &fallback,
            );
            log::debug!(
                "[frontend-shell] navigation allowed={allowed}: {}",
                navigation_identity(url)
            );
            if !allowed {
                open_external_link(url, origin.as_deref(), api_origin.as_deref());
            }
            allowed
        })
        .build()
    {
        Ok(window) => Ok(window),
        Err(error) => {
            let _ = supervisor.active_view_generation.compare_exchange(
                view_generation,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            supervisor.root_render.retire();
            Err(error.into())
        }
    }
}

fn same_navigation_document(url: &Url, expected: &Url) -> bool {
    let mut url = url.clone();
    url.set_query(None);
    url.set_fragment(None);
    url == *expected
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UnrenderedRoot {
    KeepDocument,
    PackagedFallback,
    Unavailable,
}

/// Only a shell-prepared URL carries the generation an acknowledgement must match, so a
/// document the page loaded itself (reload, update swap) can never be acknowledged.
fn unrendered_root_action(url: &Url, fallback: &Url, prepared: bool) -> UnrenderedRoot {
    if !prepared {
        UnrenderedRoot::KeepDocument
    } else if same_navigation_document(url, fallback) {
        UnrenderedRoot::Unavailable
    } else {
        UnrenderedRoot::PackagedFallback
    }
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

/// WebView reports data: URLs inconsistently, so the recovery surface is tracked explicitly.
static ON_RECOVERY_SURFACE: AtomicBool = AtomicBool::new(false);
/// The start-failure page is a recovery surface the retrying page must still replace.
static ON_START_FAILURE: AtomicBool = AtomicBool::new(false);

/// Startup keeps the window hidden until something worth seeing is painted; this bounds that wait.
const REVEAL_DEADLINE: Duration = Duration::from_secs(4);

pub(crate) fn reveal_main(app: &codeterm_client_shell::app_runtime::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(true) {
        return;
    }
    let shown = window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus());
    if let Err(error) = shown {
        log::debug!("[frontend-shell] window reveal failed: {error}");
    }
}

pub(crate) fn reveal_after_deadline(app: codeterm_client_shell::app_runtime::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REVEAL_DEADLINE).await;
        reveal_main(&app);
    });
}

fn show_unavailable(window: &WebviewWindow) {
    reveal_main(window.app_handle());
    // Re-navigating would restart the page (and its retry ring) on every failed attempt.
    let on_recovery_surface = ON_RECOVERY_SURFACE.swap(true, Ordering::AcqRel);
    if !ON_START_FAILURE.swap(false, Ordering::AcqRel) && on_recovery_surface {
        return;
    }
    match unavailable_url()
        .map_err(|error| error.to_string())
        .and_then(|url| window.navigate(url).map_err(|error| error.to_string()))
    {
        Ok(()) => {
            log::debug!("[frontend-shell] native recovery surface selected");
        }
        Err(error) => {
            ON_RECOVERY_SURFACE.store(false, Ordering::Release);
            log::error!("[frontend-shell] recovery surface failed: {error}");
        }
    }
}

/// A fragment-only change keeps the document, so a repeated refusal just updates its reason.
pub(crate) fn show_start_failure(window: &WebviewWindow, reason: &str) {
    reveal_main(window.app_handle());
    ON_RECOVERY_SURFACE.store(true, Ordering::Release);
    ON_START_FAILURE.store(true, Ordering::Release);
    match start_failure_url(Some(reason))
        .map_err(|error| error.to_string())
        .and_then(|url| window.navigate(url).map_err(|error| error.to_string()))
    {
        Ok(()) => log::debug!("[frontend-shell] start failure surface selected"),
        Err(error) => {
            ON_START_FAILURE.store(false, Ordering::Release);
            ON_RECOVERY_SURFACE.store(false, Ordering::Release);
            log::error!("[frontend-shell] start failure surface failed: {error}");
        }
    }
}

pub(crate) fn server_disconnected(app: codeterm_client_shell::app_runtime::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        show_unavailable(&window);
    }
}

const SHELL_ACTION_SCHEME: &str = "domios-shell";

/// The recovery page has no IPC, so its titlebar and Retry button navigate to `domios-shell://<action>`.
fn run_shell_action(app: &codeterm_client_shell::app_runtime::AppHandle, url: &Url) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let outcome = match url.host_str().unwrap_or_default() {
        "drag" => window.start_dragging().map_err(|error| error.to_string()),
        "minimize" => window.minimize().map_err(|error| error.to_string()),
        "maximize" => match window.is_maximized() {
            Ok(true) => window.unmaximize().map_err(|error| error.to_string()),
            Ok(false) => window.maximize().map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        },
        "close" => window.close().map_err(|error| error.to_string()),
        "retry" => {
            retry(app);
            Ok(())
        }
        other => Err(format!("unknown shell action {other}")),
    };
    if let Err(error) = outcome {
        log::debug!("[frontend-shell] shell action failed: {error}");
    }
}

fn next_generation(generation: &AtomicU64) -> Option<u64> {
    generation
        .try_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            current.checked_add(1)
        })
        .ok()?
        .checked_add(1)
}

pub(crate) fn active_view_matches(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    generation: u64,
) -> bool {
    app.try_state::<ShellSupervisor>()
        .is_some_and(|supervisor| {
            supervisor.active_view_generation.load(Ordering::Acquire) == generation
        })
}

fn prepare_root_navigation(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    url: Url,
) -> Result<Url, String> {
    let supervisor = app.state::<ShellSupervisor>();
    let view = supervisor.active_view_generation.load(Ordering::Acquire);
    if view == 0 {
        return Err("Main WebView generation is no longer active".into());
    }
    let navigation = next_generation(&supervisor.navigation)
        .ok_or_else(|| "Navigation generation space is exhausted".to_string())?;
    let generation = ClientViewGeneration { view, navigation };
    supervisor
        .root_render
        .arm(generation, url.origin().ascii_serialization());
    Ok(with_root_generation(&url, generation))
}

fn navigate_frontend(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    window: &WebviewWindow,
    url: Url,
) -> Result<(), String> {
    let prepared = prepare_root_navigation(app, url)?;
    if let Err(error) = window.navigate(prepared) {
        app.state::<ShellSupervisor>().root_render.retire();
        return Err(error.to_string());
    }
    ON_RECOVERY_SURFACE.store(false, Ordering::Release);
    crate::host_ports::native_client::startup_reached(
        crate::host_ports::native_client::ClientStartupStage::Frontend,
    );
    Ok(())
}

fn observe_navigation(
    window: WebviewWindow,
    url: Url,
    view_generation: u64,
    allowed_origin: Option<&str>,
    unavailable: &Url,
    fallback: &Url,
) {
    let app = window.app_handle();
    let supervisor = app.state::<ShellSupervisor>();
    let navigation = supervisor.navigation.clone();
    let root_render = supervisor.root_render.clone();
    let api_origin = current_api_origin(app);
    if url.scheme() == "data"
        || !navigation_allowed(
            &url,
            allowed_origin,
            api_origin.as_deref(),
            unavailable,
            fallback,
        )
    {
        root_render.retire();
        if let Some(client) =
            app.try_state::<crate::host_ports::native_client::NativeHostClientSlot>()
        {
            client.release_view();
        }
        log::debug!("[frontend-shell] root observer skipped recovery or unpermitted document");
        return;
    }
    let origin = url.origin().ascii_serialization();
    let prepared = root_generation_from_url(&url).filter(|generation| {
        generation.view == view_generation && root_render.is_armed(*generation, &origin)
    });
    let unrendered = unrendered_root_action(&url, fallback, prepared.is_some());
    let generation = prepared.or_else(|| {
        next_generation(&supervisor.navigation).map(|navigation| ClientViewGeneration {
            view: view_generation,
            navigation,
        })
    });
    let Some(generation) = generation else {
        root_render.retire();
        log::error!("[frontend-shell] navigation generation space is exhausted");
        return;
    };
    if let Some(client) = app.try_state::<crate::host_ports::native_client::NativeHostClientSlot>()
    {
        if let Err(reason) = client.bind_observed_view(&url, generation) {
            log::debug!("[native-client] main view binding refused: {reason:?}");
        }
    }
    let Some(attachment) = app
        .try_state::<ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current())
    else {
        log::debug!("[frontend-shell] boot observer skipped: server attachment is absent");
        return;
    };
    let server_identity = attachment.identity().clone();
    let server_epoch = attachment.epoch().to_owned();
    drop(attachment);
    if !root_render.is_armed(generation, &origin) {
        root_render.arm(generation, origin.clone());
        let document_generation = format!(
            "globalThis.__CODETERM_CLIENT_VIEW_GENERATION__ = Object.freeze({{viewGeneration:{view_generation},navigationGeneration:{}}});",
            generation.navigation
        );
        if let Err(error) = window.eval(&document_generation) {
            log::debug!("[frontend-shell] root generation injection failed: {error}");
        }
    }
    let current_navigation = generation.navigation;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    log::debug!("[frontend-shell] root observer armed: view={view_generation}, navigation={current_navigation}, server_epoch={server_epoch}, unrendered={unrendered:?}");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match root_render.wait_until(generation, deadline).await {
            ClientRootRenderWait::Ready => {
                log::debug!("[frontend-shell] root render acknowledged: view={view_generation}, navigation={current_navigation}");
                reveal_main(&app);
                return;
            }
            ClientRootRenderWait::Replaced => {
                log::debug!("[frontend-shell] stale root observer retired: view={view_generation}, navigation={current_navigation}");
                return;
            }
            ClientRootRenderWait::Deadline => {}
        }
        let same_server = app
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
            .is_some_and(|current| {
                current.identity() == &server_identity && current.epoch() == server_epoch
            });
        if navigation.load(Ordering::Acquire) != current_navigation || !same_server {
            log::debug!("[frontend-shell] stale boot failure ignored: navigation={current_navigation}, server_epoch={server_epoch}");
            return;
        }
        match unrendered {
            UnrenderedRoot::KeepDocument => {
                log::debug!("[frontend-shell] page-initiated document kept: navigation={current_navigation}");
                return;
            }
            UnrenderedRoot::Unavailable => {
                log::warn!("[frontend-shell] packaged root render was not acknowledged");
                show_unavailable(&window);
                return;
            }
            UnrenderedRoot::PackagedFallback => {}
        }
        log::warn!(
            "[frontend-shell] root render was not acknowledged; selecting packaged fallback"
        );
        match packaged_fallback(&app) {
            Ok(fallback) => {
                if let Err(error) = navigate_frontend(&app, &window, fallback) {
                    log::error!("[frontend-shell] packaged fallback navigation failed: {error}");
                    show_unavailable(&window);
                }
            }
            Err(error) => {
                log::error!("[frontend-shell] packaged fallback URL failed: {error}");
                show_unavailable(&window);
            }
        }
    });
}

async fn frontend_url(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    origin: &str,
) -> Result<Url, String> {
    if let Some(url) = &app.state::<ShellSupervisor>().development_frontend {
        log::debug!(
            "[frontend-shell] selecting configured development frontend: {}",
            navigation_identity(url)
        );
        // Navigating to a refused origin paints the WebView's own error page instead of ours.
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(|error| error.to_string())?;
        tokio::time::timeout(Duration::from_secs(10), client.get(url.clone()).send())
            .await
            .map_err(|_| "Development frontend exceeded its failure deadline".to_string())?
            .map_err(|error| format!("Development frontend unavailable: {error}"))?;
        return Ok(url.clone());
    }
    let target = Url::parse(origin)
        .and_then(|origin| origin.join("/bundled/index.html"))
        .map_err(|error| error.to_string())?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    readiness::wait(&client, origin).await?;
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
        packaged_fallback(app)
    }
}

pub fn start(app: codeterm_client_shell::app_runtime::AppHandle) {
    if app
        .state::<ShellSupervisor>()
        .observer_started
        .swap(true, Ordering::AcqRel)
    {
        return;
    }
    let mut windows = app.state::<ShellSupervisor>().windows.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let epoch = *windows.borrow_and_update();
            if let Some(window) = app.get_webview_window("main").filter(|_| epoch.available) {
                let attachment = app
                    .try_state::<ServerApiAttachmentSlot>()
                    .and_then(|slot| slot.current());
                if let Some(attachment) = attachment {
                    let identity = attachment.identity().clone();
                    let server_epoch = attachment.epoch().to_owned();
                    let origin = attachment.api_origin();
                    drop(attachment);
                    let target = tokio::select! {
                        target = frontend_url(&app, &origin) => target,
                        changed = windows.changed() => {
                            if changed.is_err() { return; }
                            continue;
                        },
                    };
                    match target {
                        Ok(url)
                            if same_server_attachment(&app, &identity, &server_epoch)
                                && *windows.borrow() == epoch =>
                        {
                            log::debug!("[frontend-shell] navigating after server readiness: epoch={server_epoch}, url={}", navigation_identity(&url));
                            if let Err(error) = navigate_frontend(&app, &window, url) {
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
                } else {
                    log::error!("[frontend-shell] frontend has no authenticated server attachment");
                    show_unavailable(&window);
                }
            }
            if windows.changed().await.is_err() {
                return;
            }
        }
    });
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
                if let Some(window) = app.get_webview_window("main") {
                    show_unavailable(&window);
                }
            }
        }
        Err(error) => {
            log::error!("[frontend-shell] client retry refused: {error}");
            if let Some(window) = app.get_webview_window("main") {
                if error.stated_refusal() {
                    show_start_failure(&window, error.message());
                } else {
                    show_unavailable(&window);
                }
            }
        }
    }
}

pub fn retry(app: &codeterm_client_shell::app_runtime::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(attachment) = app
            .try_state::<ServerApiAttachmentSlot>()
            .and_then(|slot| slot.current())
        else {
            reinitialize_client(app).await;
            return;
        };
        let Some(window) = app.get_webview_window("main") else {
            return;
        };
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
    let current_url = window.url().map_err(|error| error.to_string())?;
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
    if can_reuse_window {
        ensure_server_attachment(&app, expected)?;
        let url = frontend_url(&app, &loopback_origin(expected.api_address)?).await?;
        ensure_server_attachment(&app, expected)?;
        navigate_frontend(&app, &window, url)?;
        return Ok(());
    }
    if !ON_RECOVERY_SURFACE.load(Ordering::Acquire)
        && !same_navigation_document(&current_url, &unavailable)
    {
        start(app);
        return Ok(());
    }

    crate::deep_links::register_deep_links(&app).map_err(|error| error.to_string())?;
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
    if old_view == 0 || !before.available {
        return Err("Unavailable frontend generation is no longer active".into());
    }
    let close_window = window.clone();
    run_on_main_thread(app.clone(), move |handle| {
        let state = handle.state::<ShellSupervisor>();
        let live_url = close_window.url().map_err(|error| error.to_string())?;
        if (!ON_RECOVERY_SURFACE.load(Ordering::Acquire)
            && !same_navigation_document(&live_url, &unavailable))
            || state.active_view_generation.load(Ordering::Acquire) != old_view
            || state.navigation.load(Ordering::Acquire) != old_navigation
        {
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

fn ensure_server_attachment(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    expected: &codeterm_types::server_bootstrap::ServerReady,
) -> Result<(), String> {
    if app
        .try_state::<ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current())
        .is_some_and(|current| current.ready() == expected)
    {
        Ok(())
    } else {
        Err("Authenticated server attachment changed during frontend recovery".into())
    }
}

async fn run_on_main_thread<T: Send + 'static>(
    app: codeterm_client_shell::app_runtime::AppHandle,
    action: impl FnOnce(codeterm_client_shell::app_runtime::AppHandle) -> Result<T, String>
        + Send
        + 'static,
) -> Result<T, String> {
    const QUEUED: u8 = 0;
    const RUNNING: u8 = 1;
    const CANCELLED: u8 = 2;
    const COMPLETED: u8 = 3;

    let (sender, mut receiver) = tokio::sync::oneshot::channel();
    let action_app = app.clone();
    let phase = Arc::new(AtomicU8::new(QUEUED));
    let scheduled_phase = phase.clone();
    app.run_on_main_thread(move || {
        if scheduled_phase
            .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let result = action(action_app);
        scheduled_phase.store(COMPLETED, Ordering::Release);
        let _ = sender.send(result);
    })
    .map_err(|error| error.to_string())?;
    match tokio::time::timeout(Duration::from_secs(10), &mut receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            if phase
                .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                Err("Main-thread frontend action was cancelled before execution".into())
            } else {
                Err("Main-thread frontend action outcome is uncertain after execution began".into())
            }
        }
        Err(_) => {
            if phase
                .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Err("Main-thread frontend action deadline elapsed before execution".into());
            }
            match receiver.try_recv() {
                Ok(result) => result,
                Err(_) => Err("Main-thread frontend action acknowledgement is uncertain".into()),
            }
        }
    }
}

fn same_server_attachment(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    identity: &codeterm_types::server_bootstrap::ServerIdentity,
    epoch: &str,
) -> bool {
    app.try_state::<ServerApiAttachmentSlot>()
        .and_then(|slot| slot.current())
        .is_some_and(|current| current.identity() == identity && current.epoch() == epoch)
}

#[cfg(test)]
mod tests;
