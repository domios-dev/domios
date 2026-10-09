pub mod cdp;
pub mod cdp_wire;
pub mod launch;
pub mod locator;
pub mod process;
pub mod profile;
pub mod runtime;
pub mod screencast;
pub mod snapshot;
pub mod sweep;
pub mod viewport;

use parking_lot::Mutex;
use process::{CloseOutcome, ProcessExit};
use profile::{ProfileKind, ProfileLayout, ProfileLease};
use runtime::{BrowserMode, Runtime};
use screencast::{
    CaptureHold, FrameHub, HubNotice, ScreencastFrame, ScreencastParams, ScreencastSubscription,
};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use viewport::{Viewport, ViewportDecision, ViewportMode, ViewportModel, ViewportRequest};

pub use cdp::{CdpClient, CdpError, EventStream, PageChannel};

#[derive(Clone, Debug)]
pub struct BrowserHostConfig {
    pub binary: Option<PathBuf>,
    pub layout: ProfileLayout,
    pub initial_viewport: Viewport,
}

impl BrowserHostConfig {
    pub fn under_data_dir(data_dir: &Path) -> Self {
        Self {
            binary: None,
            layout: ProfileLayout::under_data_dir(data_dir),
            initial_viewport: Viewport::default(),
        }
    }
}

/// `profile` empty means a clean throwaway profile, as in `host.browser.open`.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenOptions {
    pub profile: String,
    pub width: f64,
    pub height: f64,
    pub dpr: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedBrowser {
    pub endpoint: String,
    pub profile: String,
    pub binary: Option<PathBuf>,
    pub ready_after: Option<Duration>,
    pub reused: bool,
}

impl OpenedBrowser {
    fn from_runtime(runtime: &Runtime, reused: bool) -> Self {
        Self {
            endpoint: runtime.endpoint.clone(),
            profile: runtime.mode.profile_label().to_string(),
            binary: runtime.binary.clone(),
            ready_after: runtime
                .process
                .as_ref()
                .map(|process| process.ready_after()),
            reused,
        }
    }
}

/// What to do with a running browser when `open` asks for `wanted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenPlan {
    Launch,
    Reuse,
    Relaunch,
}

pub fn plan_open(current: Option<(&BrowserMode, bool)>, wanted: &ProfileKind) -> OpenPlan {
    match current {
        None | Some((_, false)) => OpenPlan::Launch,
        Some((BrowserMode::Launched { profile }, true)) => {
            let same = match (profile, wanted) {
                (ProfileKind::Clean { .. }, ProfileKind::Clean { .. }) => true,
                (ProfileKind::Named { name: a }, ProfileKind::Named { name: b }) => a == b,
                _ => false,
            };
            if same {
                OpenPlan::Reuse
            } else {
                OpenPlan::Relaunch
            }
        }
        Some((BrowserMode::Attached { .. }, true)) => OpenPlan::Relaunch,
    }
}

/// One surface instance's browser: open/attach/close, the page channel, profiles and frames.
pub struct BrowserHost {
    config: BrowserHostConfig,
    lifecycle: tokio::sync::Mutex<()>,
    runtime: Mutex<Option<Arc<Runtime>>>,
    viewport: Mutex<ViewportModel>,
    frames: Arc<FrameHub>,
}

impl BrowserHost {
    pub fn new(config: BrowserHostConfig) -> Self {
        let initial = config.initial_viewport;
        Self {
            config,
            lifecycle: tokio::sync::Mutex::new(()),
            runtime: Mutex::new(None),
            viewport: Mutex::new(ViewportModel::new(ViewportMode::Owned, initial)),
            frames: FrameHub::new(ScreencastParams::new(initial)),
        }
    }

    fn current(&self) -> Option<Arc<Runtime>> {
        self.runtime.lock().clone()
    }

    fn install(&self, runtime: Runtime) -> Arc<Runtime> {
        let mode = match runtime.mode {
            BrowserMode::Launched { .. } => ViewportMode::Owned,
            BrowserMode::Attached { .. } => ViewportMode::Attached,
        };
        let viewport = {
            let mut model = self.viewport.lock();
            *model = ViewportModel::new(mode, model.current());
            model.current()
        };
        self.frames.set_params(ScreencastParams::new(viewport));
        let runtime = Arc::new(runtime);
        *self.runtime.lock() = Some(runtime.clone());
        runtime
    }

    async fn stop_current(&self) -> Option<CloseOutcome> {
        let previous = self.runtime.lock().take();
        match previous {
            Some(runtime) => runtime::stop(runtime).await,
            None => None,
        }
    }

    /// Daemon exit: like `shutdown`, but Chromium gets only `graceful` to flush before the kill.
    pub async fn shutdown_within(&self, graceful: Duration) -> Option<CloseOutcome> {
        let outcome = {
            let _lifecycle = self.lifecycle.lock().await;
            let previous = self.runtime.lock().take();
            match previous {
                Some(runtime) => runtime::stop_within(runtime, graceful).await,
                None => None,
            }
        };
        self.frames.close();
        outcome
    }

    async fn launch(&self, profile: ProfileKind) -> Result<Arc<Runtime>, String> {
        let viewport = self.viewport.lock().current();
        self.frames.set_params(ScreencastParams::new(viewport));
        let runtime = runtime::launch(
            self.config.binary.clone(),
            &self.config.layout,
            profile,
            viewport,
            self.frames.clone(),
        )
        .await?;
        Ok(self.install(runtime))
    }

    /// Start or reuse this instance's Chromium; a different profile relaunches it.
    pub async fn open(&self, options: OpenOptions) -> Result<OpenedBrowser, String> {
        let _lifecycle = self.lifecycle.lock().await;
        let wanted = ProfileKind::from_request(&options.profile);
        let current = self.current();
        let plan = plan_open(
            current
                .as_ref()
                .map(|runtime| (&runtime.mode, runtime.is_alive())),
            &wanted,
        );
        if let (OpenPlan::Reuse, Some(runtime)) = (&plan, &current) {
            self.resize(runtime, viewport_request(&options)).await?;
            return Ok(OpenedBrowser::from_runtime(runtime, true));
        }
        drop(current);
        self.stop_current().await;
        {
            let mut model = self.viewport.lock();
            *model = ViewportModel::new(ViewportMode::Owned, model.current());
            model.apply(viewport_request(&options));
        }
        let runtime = self.launch(wanted).await?;
        Ok(OpenedBrowser::from_runtime(&runtime, false))
    }

    /// Drive an existing CDP endpoint (e.g. a WebView2 app) instead of launching; never resizes it.
    pub async fn attach(&self, http_base: &str) -> Result<OpenedBrowser, String> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_current().await;
        let runtime = runtime::attach(http_base, self.frames.clone()).await?;
        let runtime = self.install(runtime);
        Ok(OpenedBrowser::from_runtime(&runtime, false))
    }

    /// Graceful close (Browser.close flushes cookies), then kill; viewers stay subscribed.
    pub async fn close(&self) -> Option<CloseOutcome> {
        let _lifecycle = self.lifecycle.lock().await;
        self.stop_current().await
    }

    /// Owner or Team teardown: close the browser and end every frame subscription.
    pub async fn shutdown(&self) -> Option<CloseOutcome> {
        let outcome = self.close().await;
        self.frames.close();
        outcome
    }

    /// Relaunch under `name` (`""` = clean) and reopen the current web page there.
    pub async fn use_profile(&self, name: &str) -> Result<OpenedBrowser, String> {
        let _lifecycle = self.lifecycle.lock().await;
        let wanted = ProfileKind::from_request(name);
        let current = self.current();
        if let Some(runtime) = &current {
            if matches!(runtime.mode, BrowserMode::Attached { .. }) {
                return Err("profiles are not available in attach mode".into());
            }
        }
        let alive = current
            .as_ref()
            .map(|runtime| (&runtime.mode, runtime.is_alive()));
        if let (OpenPlan::Reuse, Some(runtime)) = (plan_open(alive, &wanted), &current) {
            return Ok(OpenedBrowser::from_runtime(runtime, true));
        }
        let carry = match &current {
            Some(runtime) => runtime::current_url(runtime)
                .await
                .and_then(|url| runtime::carry_url(&url)),
            None => None,
        };
        drop(current);
        self.stop_current().await;
        let runtime = self.launch(wanted).await?;
        if let Some(url) = carry {
            if let Err(error) = runtime
                .page
                .send("Page.navigate", json!({ "url": url }))
                .await
            {
                log::warn!("[browser] reopening {url} after profile switch failed: {error}");
            }
        }
        Ok(OpenedBrowser::from_runtime(&runtime, false))
    }

    /// Named persistent profiles; user-only dirs, never sent over mesh or into mem.
    pub async fn profiles(&self) -> Vec<String> {
        let layout = self.config.layout.clone();
        tokio::task::spawn_blocking(move || layout.list_named())
            .await
            .unwrap_or_default()
    }

    /// The page-scoped CDP channel; Domios's own WebView is never reachable through it.
    pub fn page(&self) -> Option<PageChannel> {
        self.current().map(|runtime| runtime.page.clone())
    }

    pub fn endpoint(&self) -> Option<String> {
        self.current().map(|runtime| runtime.endpoint.clone())
    }

    pub fn mode(&self) -> Option<BrowserMode> {
        self.current().map(|runtime| runtime.mode.clone())
    }

    /// Process exit for crash detection; `None` in attach mode or when nothing runs.
    pub fn exit_watch(&self) -> Option<watch::Receiver<Option<ProcessExit>>> {
        self.current()
            .and_then(|runtime| runtime.process.as_ref().map(|process| process.exit_watch()))
    }

    pub fn subscribe_screencast(&self) -> ScreencastSubscription {
        self.frames.subscribe()
    }

    pub fn latest_frame(&self) -> Option<Arc<ScreencastFrame>> {
        self.frames.latest()
    }

    pub fn hold_screencast_for_capture(&self) -> CaptureHold {
        self.frames.hold_for_capture()
    }

    pub fn notices(&self) -> broadcast::Receiver<HubNotice> {
        self.frames.notices()
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport.lock().current()
    }

    /// Viewer-driven size (CSS px + DPR); attach mode refuses, as the app owns its window.
    pub async fn set_viewport(&self, request: ViewportRequest) -> Result<ViewportDecision, String> {
        match self.current() {
            Some(runtime) => self.resize(&runtime, request).await,
            None => Ok(self.viewport.lock().apply(request)),
        }
    }

    async fn resize(
        &self,
        runtime: &Runtime,
        request: ViewportRequest,
    ) -> Result<ViewportDecision, String> {
        let decision = self.viewport.lock().apply(request);
        if let ViewportDecision::Resize(viewport) = decision {
            runtime::apply_viewport(&runtime.client, &runtime.page, runtime.window_id, viewport)
                .await?;
            self.frames.set_params(ScreencastParams::new(viewport));
        }
        Ok(decision)
    }
}

fn viewport_request(options: &OpenOptions) -> ViewportRequest {
    ViewportRequest {
        width: options.width,
        height: options.height,
        dpr: options.dpr,
    }
}

/// Startup sweep for browsers a crashed daemon left behind; leased (live) profiles are kept.
pub async fn sweep_orphans(layout: ProfileLayout) -> sweep::SweepReport {
    let keep = ProfileLease::leased_dirs();
    tokio::task::spawn_blocking(move || sweep::sweep_orphans_blocking(&layout, &keep))
        .await
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> ProfileKind {
        ProfileKind::Clean {
            session_id: "a".into(),
        }
    }

    fn named(name: &str) -> ProfileKind {
        ProfileKind::Named { name: name.into() }
    }

    #[test]
    fn browser_host_open_plan_reuses_matching_live_browser() {
        let running = BrowserMode::Launched {
            profile: ProfileKind::Clean {
                session_id: "b".into(),
            },
        };
        assert_eq!(plan_open(Some((&running, true)), &clean()), OpenPlan::Reuse);
        assert_eq!(
            plan_open(Some((&running, true)), &named("work")),
            OpenPlan::Relaunch
        );
        assert_eq!(
            plan_open(Some((&running, false)), &clean()),
            OpenPlan::Launch
        );
        assert_eq!(plan_open(None, &named("work")), OpenPlan::Launch);
    }

    #[test]
    fn browser_host_open_plan_switches_between_named_profiles_and_off_attach() {
        let work = BrowserMode::Launched {
            profile: named("work"),
        };
        assert_eq!(
            plan_open(Some((&work, true)), &named("work")),
            OpenPlan::Reuse
        );
        assert_eq!(
            plan_open(Some((&work, true)), &named("home")),
            OpenPlan::Relaunch
        );
        let attached = BrowserMode::Attached {
            http_base: "http://127.0.0.1:9223".into(),
        };
        assert_eq!(
            plan_open(Some((&attached, true)), &clean()),
            OpenPlan::Relaunch
        );
    }

    #[tokio::test]
    async fn browser_host_without_runtime_tracks_viewport_and_has_no_page() {
        let root = tempfile::tempdir().unwrap();
        let host = BrowserHost::new(BrowserHostConfig::under_data_dir(root.path()));
        assert!(host.page().is_none());
        assert!(host.endpoint().is_none());
        assert!(host.close().await.is_none());
        let decision = host
            .set_viewport(ViewportRequest {
                width: 1280.0,
                height: 720.0,
                dpr: 2.0,
            })
            .await
            .unwrap();
        assert!(matches!(decision, ViewportDecision::Resize(_)));
        assert_eq!(host.viewport().dpr, 2.0);
        assert!(host.profiles().await.is_empty());
    }
}
