use super::*;

pub(crate) const SCHEME: &str = "domios-startup";

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
