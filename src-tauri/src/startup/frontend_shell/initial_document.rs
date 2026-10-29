use super::*;

pub(crate) const SCHEME: &str = "domios-startup";
pub(crate) const STARTING_READY_SCHEME: &str = "domios-starting-ready";
pub(crate) const STARTING_UNAVAILABLE_SCHEME: &str = "domios-starting-unavailable";
pub(crate) const STARTING_READY_SCRIPT: &str = include_str!("initial_document.js");

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum StartingPhase {
    #[default]
    Awaiting,
    Ready,
    Failed,
    RecoveryOffered,
}

#[derive(Clone, Copy, Default)]
struct StartingState {
    generation: u64,
    phase: StartingPhase,
}

pub(crate) struct StartingLayout(tokio::sync::watch::Sender<StartingState>);

impl Default for StartingLayout {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(StartingState::default()).0)
    }
}

impl StartingLayout {
    pub(crate) fn begin(&self) -> u64 {
        let mut generation = 0;
        self.0.send_modify(|state| {
            state.generation += 1;
            state.phase = StartingPhase::Awaiting;
            generation = state.generation;
        });
        generation
    }

    pub(crate) fn retire(&self) {
        self.0.send_replace(StartingState::default());
    }

    pub(crate) fn generation(&self) -> u64 {
        self.0.borrow().generation
    }

    #[cfg(test)]
    fn acknowledge(&self, url: &Url) -> bool {
        self.resolve(url, STARTING_READY_SCHEME, StartingPhase::Ready)
            .is_some()
    }

    pub(crate) fn refuse(&self, url: &Url) -> Option<u64> {
        self.resolve(url, STARTING_UNAVAILABLE_SCHEME, StartingPhase::Failed)
    }

    fn resolve(&self, url: &Url, scheme: &str, next: StartingPhase) -> Option<u64> {
        let mut resolved = None;
        self.0.send_if_modified(|state| {
            if state.generation == 0
                || state.phase != StartingPhase::Awaiting
                || url.scheme() != scheme
                || url.host_str() != Some("localhost")
                || url.path() != format!("/{}", state.generation)
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
            {
                return false;
            }
            state.phase = next;
            resolved = Some(state.generation);
            true
        });
        resolved
    }

    fn awaiting(&self, generation: u64) -> bool {
        let state = self.0.borrow();
        generation != 0 && state.generation == generation && state.phase == StartingPhase::Awaiting
    }

    fn fail(&self, generation: u64, awaiting_only: bool) -> bool {
        self.0.send_if_modified(|state| {
            let may_fail = state.phase == StartingPhase::Awaiting
                || (!awaiting_only && state.phase == StartingPhase::Ready);
            if generation == 0 || state.generation != generation || !may_fail {
                return false;
            }
            state.phase = StartingPhase::Failed;
            true
        })
    }

    pub(crate) fn request<E>(
        &self,
        generation: u64,
        evaluate: impl FnOnce() -> Result<(), E>,
    ) -> bool {
        self.awaiting(generation) && evaluate().is_err() && self.fail(generation, false)
    }

    pub(crate) fn reveal<E>(&self, url: &Url, show: impl FnOnce() -> Result<(), E>) -> Option<u64> {
        let generation = self.resolve(url, STARTING_READY_SCHEME, StartingPhase::Ready)?;
        (show().is_err() && self.fail(generation, false)).then_some(generation)
    }

    fn claim_recovery(&self, generation: u64) -> bool {
        self.0.send_if_modified(|state| {
            if generation == 0
                || state.generation != generation
                || state.phase != StartingPhase::Failed
            {
                return false;
            }
            state.phase = StartingPhase::RecoveryOffered;
            true
        })
    }

    fn recovery_active(&self, generation: u64) -> bool {
        let state = self.0.borrow();
        generation != 0
            && state.generation == generation
            && matches!(
                state.phase,
                StartingPhase::Failed | StartingPhase::RecoveryOffered
            )
    }

    async fn resolved(&self, generation: u64) {
        let mut changed = self.0.subscribe();
        loop {
            let state = *changed.borrow_and_update();
            if state.generation != generation || state.phase != StartingPhase::Awaiting {
                return;
            }
            if changed.changed().await.is_err() {
                return;
            }
        }
    }

    async fn observe_failure(
        &self,
        generation: u64,
        deadline: impl std::future::Future<Output = Result<(), ()>>,
    ) -> bool {
        deadline.await.is_err() && self.fail(generation, true)
    }
}

pub(crate) fn supervise_starting(
    app: codeterm_client_shell::app_runtime::AppHandle,
    layout: Arc<StartingLayout>,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        let deadline = async {
            tokio::time::timeout(Duration::from_secs(30), layout.resolved(generation))
                .await
                .map_err(|_| ())
        };
        if layout.observe_failure(generation, deadline).await {
            show_layout_failure(&app, layout.clone(), generation);
        }
    });
}

pub(crate) fn show_layout_failure(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    layout: Arc<StartingLayout>,
    generation: u64,
) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        if handle.get_webview_window("starting").is_none() || !layout.claim_recovery(generation) {
            return;
        }
        let retry_app = handle.clone();
        handle
            .dialog()
            .message("The Domios startup window could not prepare its content. Retry to load it again, or quit.")
            .title("Domios could not start")
            .kind(MessageDialogKind::Error)
            .buttons(MessageDialogButtons::OkCancelCustom("Retry".into(), "Quit".into()))
            .show(move |retry_requested| {
                if retry_app.get_webview_window("starting").is_none()
                    || !layout.recovery_active(generation)
                {
                    return;
                }
                if retry_requested {
                    super::retry(&retry_app);
                } else {
                    let _ = crate::run::request_client_quit_exact(&retry_app, None);
                }
            });
    }) {
        log::error!("[startup] native document failure surface unavailable: {error}");
    }
}

pub(super) fn url(http_protocol: bool) -> Result<Url, url::ParseError> {
    Url::parse(if http_protocol {
        "http://domios-startup.localhost/starting"
    } else {
        "domios-startup://localhost/starting"
    })
}

pub(crate) fn window_config(
    config: &tauri::utils::config::Config,
) -> Result<tauri::utils::config::WindowConfig, Box<dyn std::error::Error>> {
    let mut window = config
        .app
        .windows
        .first()
        .cloned()
        .ok_or("Main window configuration is missing")?;
    window.url = WebviewUrl::CustomProtocol(starting_url()?);
    Ok(window)
}

pub(crate) fn starting_window_config(
    config: &tauri::utils::config::Config,
) -> Result<tauri::utils::config::WindowConfig, Box<dyn std::error::Error>> {
    let mut window = window_config(config)?;
    window.label = "starting".into();
    window.visible = false;
    window.transparent = false;
    Ok(window)
}

pub(crate) fn protocol_response() -> tauri::http::Response<Vec<u8>> {
    let mut response =
        tauri::http::Response::new(brand_fallback_page(BrandFallback::Starting).into_bytes());
    response.headers_mut().insert(
        tauri::http::header::CONTENT_TYPE,
        tauri::http::HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Absent,
    Awaiting(u64),
    Ready(u64),
    Failed(u64),
}

pub(super) struct InitialDocument(tokio::sync::watch::Sender<Phase>);

impl Default for InitialDocument {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(Phase::Absent).0)
    }
}

impl InitialDocument {
    pub(super) fn begin(&self, view: u64) {
        self.0.send_replace(Phase::Awaiting(view));
    }

    pub(super) fn acknowledge(&self, view: u64) -> bool {
        self.transition(view, Phase::Ready(view))
    }

    fn transition(&self, view: u64, next: Phase) -> bool {
        self.0.send_if_modified(|phase| {
            if *phase != Phase::Awaiting(view) {
                return false;
            }
            *phase = next;
            true
        })
    }

    pub(super) fn failed(&self, view: u64) -> bool {
        *self.0.borrow() == Phase::Failed(view)
    }

    pub(super) fn retire(&self) {
        self.0.send_replace(Phase::Absent);
    }

    async fn resolved(&self, view: u64) {
        let mut changed = self.0.subscribe();
        loop {
            if *changed.borrow_and_update() != Phase::Awaiting(view) {
                return;
            }
            if changed.changed().await.is_err() {
                return;
            }
        }
    }

    async fn observe_failure(
        &self,
        view: u64,
        result: impl std::future::Future<Output = Result<(), ()>>,
    ) -> bool {
        result.await.is_err() && self.transition(view, Phase::Failed(view))
    }
}

pub(super) fn supervise(app: codeterm_client_shell::app_runtime::AppHandle, view: u64) {
    tauri::async_runtime::spawn(async move {
        let supervisor = app.state::<ShellSupervisor>();
        let initial = &supervisor.initial_document;
        let deadline = async {
            tokio::time::timeout(Duration::from_secs(30), initial.resolved(view))
                .await
                .map_err(|_| ())
        };
        if !initial.observe_failure(view, deadline).await
            || !active_view_matches(&app, view)
            || crate::crash::exit::is_client_exit_pending()
        {
            return;
        }
        let result = run_on_main_thread(app.clone(), move |handle| {
            if active_view_matches(&handle, view)
                && handle.state::<ShellSupervisor>().initial_document.failed(view)
                && !crate::crash::exit::is_client_exit_pending()
            {
                if let Some(window) = handle.get_webview_window("starting")
                    .or_else(|| handle.get_webview_window("main"))
                {
                    show_start_failure(&window, "The initial Domios window did not finish loading. Retry to create a new window.");
                }
            }
            Ok(())
        }).await;
        if let Err(error) = result {
            log::error!("[frontend-shell] initial-document failure surface unavailable: {error}");
        }
    });
}

#[cfg(test)]
mod tests;
