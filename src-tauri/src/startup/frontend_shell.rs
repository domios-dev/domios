use codeterm_client_shell::app_runtime::WebviewWindow;
#[cfg(debug_assertions)]
mod assets;
pub(crate) mod initial_document;
mod main_thread;
mod readiness;
mod reconnect;
mod recovery;
mod surfaces;
use main_thread::run_on_main_thread;
pub(crate) use reconnect::cancel as cancel_recovery;
pub(crate) use recovery::{recover_for_attachment, retry};
use surfaces::show_unavailable;
pub(crate) use surfaces::{reveal_main, show_start_failure};

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
    webview_diagnostics::{root_generation_from_url, with_root_generation, PortPreflight},
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
        || same_origin(url, api_origin)
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
    diagnostics_preflight: StartupDiagnosticsPreflight,
    windows: tokio::sync::watch::Sender<WindowEpoch>,
    capability_origins: parking_lot::Mutex<HashSet<String>>,
    view_bootstrap: parking_lot::Mutex<Option<ShellBootstrap>>,
    initial_document: initial_document::InitialDocument,
    reconnect: reconnect::RecoveryOwner,
}

struct StartupDiagnosticsPreflight(Option<(u16, PortPreflight)>);

impl StartupDiagnosticsPreflight {
    fn capture(
        mode: crate::LocalWebviewDiagnosticsMode,
        probe: impl FnOnce(u16) -> PortPreflight,
    ) -> Self {
        Self(match mode {
            crate::LocalWebviewDiagnosticsMode::Disabled => None,
            crate::LocalWebviewDiagnosticsMode::RequestedPort(port) => Some((port, probe(port))),
        })
    }

    fn for_port(&self, port: u16) -> Result<PortPreflight, &'static str> {
        match self.0 {
            Some((requested, preflight)) if requested == port => Ok(preflight),
            Some(_) => Err("Native diagnostics port does not match startup preflight"),
            None => Err("Native diagnostics startup preflight is missing"),
        }
    }
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

pub(super) fn starting_url() -> Result<Url, url::ParseError> {
    initial_document::url(cfg!(any(windows, target_os = "android")))
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
        || ((same_origin(url, origin) || same_origin(url, api_origin))
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
    initialize_with_preflight(app, |port| {
        let preflight = crate::host_ports::webview_diagnostics::port_preflight(port);
        #[cfg(debug_assertions)]
        let preflight =
            preflight.with_starting_view_present(app.get_webview_window("starting").is_some());
        preflight
    })
}

fn initialize_with_preflight(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    probe: impl FnOnce(u16) -> PortPreflight,
) -> Result<(), Box<dyn std::error::Error>> {
    if app.try_state::<ShellSupervisor>().is_some() {
        return Err("Frontend shell is already initialized".into());
    }
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
    let diagnostics_preflight = StartupDiagnosticsPreflight::capture(
        app.state::<crate::ClientStartupOptions>().local_webview_diagnostics,
        probe,
    );
    app.manage(ShellSupervisor {
        development_frontend,
        diagnostics_preflight,
        navigation: Arc::new(AtomicU64::new(0)),
        view_generation: AtomicU64::new(0),
        active_view_generation: AtomicU64::new(0),
        observer_started: std::sync::atomic::AtomicBool::new(false),
        root_render,
        windows: tokio::sync::watch::channel(WindowEpoch::default()).0,
        capability_origins: Default::default(),
        view_bootstrap: Default::default(),
        initial_document: Default::default(),
        reconnect: Default::default(),
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
    let active_view = supervisor.active_view_generation.swap(0, Ordering::AcqRel);
    supervisor.initial_document.retire();
    supervisor.root_render.retire();
    supervisor.windows.send_if_modified(|epoch| {
        if !epoch.available && active_view == 0 {
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
    let mut window_config = initial_document::window_config(app.config())?;
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
    let diagnostics_preflight = match app
        .state::<crate::ClientStartupOptions>()
        .local_webview_diagnostics
    {
        crate::LocalWebviewDiagnosticsMode::Disabled => None,
        crate::LocalWebviewDiagnosticsMode::RequestedPort(port) => {
            Some((port, supervisor.diagnostics_preflight.for_port(port)?))
        }
    };
    let shell_app = app.clone();
    let mut builder = WebviewWindowBuilder::from_config(app, &window_config)?;
    if let Some((port, preflight)) = diagnostics_preflight {
        if let Some(slot) =
            app.try_state::<crate::host_ports::native_client::NativeHostClientSlot>()
        {
            slot.set_diagnostics_port_preflight(port, preflight);
        }
        #[cfg(windows)]
        if preflight.available() {
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
    supervisor.initial_document.begin(view_generation);
    match builder
        .on_new_window(move |url, _features| {
            let api_origin = current_api_origin(&new_window_app);
            open_external_link(&url, new_window_origin.as_deref(), api_origin.as_deref());
            tauri::webview::NewWindowResponse::Deny
        })
        .on_page_load(move |window, payload| {
            if !active_view_matches(window.app_handle(), view_generation) {
                return;
            }
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
            } else {
                if starting_document_finished(payload.event(), payload.url())
                    && window
                        .app_handle()
                        .state::<ShellSupervisor>()
                        .initial_document
                        .acknowledge(view_generation)
                {
                    window_created(window.app_handle());
                }
                if cfg!(debug_assertions) {
                    let _ = window.eval("window.__CODETERM_SHELL_DIAGNOSTIC__?.('page-finished')");
                }
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
        Ok(window) => {
            initial_document::supervise(app.clone(), view_generation);
            Ok(window)
        }
        Err(error) => {
            let _ = supervisor.active_view_generation.compare_exchange(
                view_generation,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            supervisor.root_render.retire();
            supervisor.initial_document.retire();
            Err(error.into())
        }
    }
}

fn starting_document_finished(event: tauri::webview::PageLoadEvent, url: &Url) -> bool {
    event == tauri::webview::PageLoadEvent::Finished
        && starting_url().is_ok_and(|starting| same_navigation_document(url, &starting))
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

pub(crate) fn server_disconnected(
    app: codeterm_client_shell::app_runtime::AppHandle,
    ready: codeterm_types::server_bootstrap::ServerReady,
    process: Arc<codeterm_process_identity::PinnedProcess>,
) {
    if let Some(window) = app.get_webview_window("main") {
        show_unavailable(&window);
    }
    if !crate::crash::exit::is_client_exit_pending() {
        reconnect::start(app, ready, process);
    }
}

pub(super) const SHELL_ACTION_SCHEME: &str = "domios-shell";

/// The recovery page has no IPC, so its titlebar and Retry button navigate to `domios-shell://<action>`.
pub(super) fn run_shell_action(app: &codeterm_client_shell::app_runtime::AppHandle, url: &Url) {
    let Some(window) = app
        .get_webview_window("main")
        .filter(|window| window.is_visible().unwrap_or(false))
        .or_else(|| app.get_webview_window("starting"))
    else {
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
        "quit" => {
            crate::run::request_client_quit_exact(app, None).map_err(|error| format!("{error:?}"))
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

fn active_root_matches(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    generation: ClientViewGeneration,
) -> bool {
    app.try_state::<ShellSupervisor>()
        .is_some_and(|supervisor| {
            supervisor.active_view_generation.load(Ordering::Acquire) == generation.view
                && supervisor.navigation.load(Ordering::Acquire) == generation.navigation
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
    if !active_view_matches(app, view_generation) {
        return;
    }
    let supervisor = app.state::<ShellSupervisor>();
    let navigation = supervisor.navigation.clone();
    let root_render = supervisor.root_render.clone();
    let api_origin = current_api_origin(app);
    if url.scheme() == "data"
        || starting_url().is_ok_and(|starting| same_navigation_document(&url, &starting))
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
                let reveal = run_on_main_thread(app.clone(), move |handle| {
                    if active_root_matches(&handle, generation)
                        && same_server_attachment(&handle, &server_identity, &server_epoch)
                    {
                        log::debug!("[frontend-shell] root render acknowledged: view={view_generation}, navigation={current_navigation}");
                        crate::host_ports::native_client::startup_reached(
                            crate::host_ports::native_client::ClientStartupStage::RootRender,
                        );
                        reveal_main(&handle);
                        crate::host_ports::native_client::startup_reached(
                            crate::host_ports::native_client::ClientStartupStage::DeskInteractive,
                        );
                        cancel_recovery(&handle);
                    }
                    Ok(())
                }).await;
                if let Err(error) = reveal {
                    log::error!("[frontend-shell] root reveal failed: {error}");
                    if let Some(window) = app.get_webview_window("starting") {
                        show_start_failure(&window, &error);
                    }
                }
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
    readiness::wait(&client, origin)
        .await
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
