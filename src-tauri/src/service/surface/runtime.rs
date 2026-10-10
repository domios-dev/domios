use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_core::Stream;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use super::model::SurfaceError;
use crate::service::browser::screencast::ScreencastFrame;
use crate::service::browser::viewport::ViewportRequest;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SurfaceState {
    pub owner: String,
    pub title: String,
    pub url: String,
    pub activity: String,
    /// The agent driving `activity` when it is not the surface owner.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub actor: String,
    pub needs_human: String,
    pub profile: String,
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NoticeTone {
    Info,
    Error,
}

/// Lets the viewer render a recurring condition in place instead of as a notice bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoticeKind {
    PreviewPaused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SurfaceNotice {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<NoticeTone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<NoticeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub(crate) const PREVIEW_PAUSED_TEXT: &str = "Preview paused while the window is hidden";

impl SurfaceNotice {
    fn toned(text: impl Into<String>, tone: Option<NoticeTone>) -> Self {
        Self {
            text: text.into(),
            tone,
            kind: None,
            detail: None,
        }
    }

    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self::toned(text, None)
    }

    pub(crate) fn info(text: impl Into<String>) -> Self {
        Self::toned(text, Some(NoticeTone::Info))
    }

    pub(crate) fn error(text: impl Into<String>) -> Self {
        Self::toned(text, Some(NoticeTone::Error))
    }

    pub(crate) fn preview_paused(detail: impl Into<String>) -> Self {
        Self {
            kind: Some(NoticeKind::PreviewPaused),
            detail: Some(detail.into()),
            ..Self::plain(PREVIEW_PAUSED_TEXT)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SurfaceEvent {
    State(SurfaceState),
    Frame(Arc<ScreencastFrame>),
    Notice(SurfaceNotice),
}

/// Where a relaunched surface picks up: its last page and, for the browser, its profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SurfaceResume {
    pub url: Option<String>,
    pub profile: Option<String>,
}

impl SurfaceResume {
    pub(crate) fn is_empty(&self) -> bool {
        self.url.is_none() && self.profile.is_none()
    }
}

/// Dropping the stream disconnects the viewer; the last viewer leaving stops frame production.
pub(crate) type SurfaceEvents = Pin<Box<dyn Stream<Item = SurfaceEvent> + Send>>;

/// Who a call acts for: the surface owner, whose pad receives evidence files when it exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceCallCtx {
    pub owner_tab_id: Option<String>,
    pub data_dir: PathBuf,
    /// The calling agent's name when it is not the owner.
    pub actor: Option<String>,
}

impl SurfaceCallCtx {
    /// Blocking: probes the owner's pad on disk.
    pub(crate) fn evidence_dir(&self) -> PathBuf {
        evidence_dir(&self.data_dir, self.owner_tab_id.as_deref())
    }
}

pub(crate) fn evidence_dir(data_dir: &Path, owner_tab_id: Option<&str>) -> PathBuf {
    owner_tab_id
        .and_then(|owner| crate::service::agent_pad::AgentPad::open(data_dir, owner).ok())
        .map(|pad| pad.path().join("screenshots"))
        .unwrap_or_else(|| data_dir.join("browsers").join("screenshots"))
}

/// Side effects a runtime asks core to perform after a call; it never reaches back into core itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SurfaceEffect {
    DeliverAnnotation { id: String, markdown: String },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceCallOutcome {
    pub result: Value,
    pub effects: Vec<SurfaceEffect>,
}

impl SurfaceCallOutcome {
    pub(crate) fn value(result: Value) -> Self {
        Self {
            result,
            effects: Vec::new(),
        }
    }
}

/// One live instance per stream surface tab, created lazily and disposed with the tab.
#[async_trait::async_trait]
pub(crate) trait SurfaceRuntime: Send + Sync {
    fn set_owner_name(&self, name: &str);
    #[cfg(test)]
    fn state(&self) -> SurfaceState;
    /// Applies `viewport` before any frame reaches the returned stream.
    async fn connect(self: Arc<Self>, viewport: Option<ViewportRequest>) -> SurfaceEvents;
    async fn call(
        &self,
        ctx: &SurfaceCallCtx,
        method: &str,
        args: Value,
    ) -> Result<SurfaceCallOutcome, String>;
    async fn agent_verb(
        &self,
        ctx: &SurfaceCallCtx,
        verb: &str,
        args: &[String],
    ) -> Result<String, String>;
    /// An agent's annotation of an element for the surface owner; `ctx.actor` is its sender.
    async fn annotate(
        &self,
        ctx: &SurfaceCallCtx,
        args: &[String],
    ) -> Result<SurfaceCallOutcome, String> {
        let _ = (ctx, args);
        Err("this surface cannot annotate".into())
    }
    fn annotation_delivered(&self, id: &str);
    /// Resume points worth persisting; `None` when the runtime has nothing to resume.
    fn resume_updates(&self) -> Option<tokio::sync::watch::Receiver<SurfaceResume>> {
        None
    }
    async fn dispose(&self);
    /// Daemon exit: dispose with only `graceful` before a kill; false when something was killed.
    async fn shutdown(&self, graceful: Duration) -> bool {
        let _ = graceful;
        self.dispose().await;
        true
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ShutdownReport {
    pub graceful: usize,
    pub killed: usize,
}

/// Closes every runtime at once; one still running at `limit` counts as killed, its job reaps it.
pub(crate) async fn shutdown_all(
    runtimes: Vec<Arc<dyn SurfaceRuntime>>,
    graceful: Duration,
    limit: Duration,
) -> ShutdownReport {
    let closing = runtimes.into_iter().map(|runtime| async move {
        tokio::time::timeout(limit, runtime.shutdown(graceful))
            .await
            .unwrap_or(false)
    });
    let closed = futures_util::future::join_all(closing).await;
    let graceful = closed.iter().filter(|closed| **closed).count();
    ShutdownReport {
        graceful,
        killed: closed.len() - graceful,
    }
}

/// The SSE event name and JSON `data` line for one stream event.
pub(crate) fn sse_payload(event: &SurfaceEvent) -> (&'static str, String) {
    use base64::Engine;
    match event {
        SurfaceEvent::State(state) => ("state", serde_json::to_string(state).unwrap_or_default()),
        SurfaceEvent::Frame(frame) => (
            "frame",
            serde_json::json!({
                "data": base64::engine::general_purpose::STANDARD.encode(&frame.jpeg),
                "metadata": frame.metadata,
            })
            .to_string(),
        ),
        SurfaceEvent::Notice(notice) => {
            ("notice", serde_json::to_string(notice).unwrap_or_default())
        }
    }
}

pub(crate) trait SurfaceRuntimeFactory: Send + Sync {
    /// Must stay cheap: nothing launches until the runtime is first used, which applies `resume`.
    fn create(&self, owner_name: &str, resume: Option<SurfaceResume>) -> Arc<dyn SurfaceRuntime>;
}

struct Instance {
    runtime_name: String,
    runtime: Arc<dyn SurfaceRuntime>,
}

/// Core surface runtimes keyed by the name a manifest declares, and the live instance per tab.
pub(crate) struct SurfaceRuntimeRegistry {
    factories: HashMap<String, Arc<dyn SurfaceRuntimeFactory>>,
    instances: Mutex<HashMap<String, Instance>>,
}

impl SurfaceRuntimeRegistry {
    pub(crate) fn new(factories: HashMap<String, Arc<dyn SurfaceRuntimeFactory>>) -> Self {
        Self {
            factories,
            instances: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn with_builtin_runtimes(data_dir: &Path) -> Self {
        let mut factories: HashMap<String, Arc<dyn SurfaceRuntimeFactory>> = HashMap::new();
        factories.insert(
            super::browser::BROWSER_RUNTIME.to_string(),
            Arc::new(super::browser::BrowserRuntimeFactory::new(data_dir)),
        );
        Self::new(factories)
    }

    pub(crate) fn runtime_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.factories.keys().cloned().collect();
        names.sort();
        names
    }

    /// The tab's live instance, or a new one seeded with `resume`; the flag says it was created.
    pub(crate) fn get_or_create(
        &self,
        tab_id: &str,
        runtime_name: &str,
        owner_name: &str,
        resume: Option<SurfaceResume>,
    ) -> Result<(Arc<dyn SurfaceRuntime>, bool), SurfaceError> {
        let factory = self
            .factories
            .get(runtime_name)
            .ok_or_else(|| SurfaceError::RuntimeUnavailable(runtime_name.to_string()))?;
        let mut instances = self.instances.lock();
        if let Some(instance) = instances
            .get(tab_id)
            .filter(|instance| instance.runtime_name == runtime_name)
        {
            return Ok((instance.runtime.clone(), false));
        }
        let runtime = factory.create(owner_name, resume);
        instances.insert(
            tab_id.to_string(),
            Instance {
                runtime_name: runtime_name.to_string(),
                runtime: runtime.clone(),
            },
        );
        Ok((runtime, true))
    }

    /// Removes every instance; the caller disposes what it gets back.
    pub(crate) fn take_all(&self) -> Vec<Arc<dyn SurfaceRuntime>> {
        self.instances
            .lock()
            .drain()
            .map(|(_, instance)| instance.runtime)
            .collect()
    }

    /// Removes every instance whose tab is gone; the caller disposes what it gets back.
    pub(crate) fn take_closed(
        &self,
        is_live: impl Fn(&str) -> bool,
    ) -> Vec<Arc<dyn SurfaceRuntime>> {
        let tab_ids: Vec<String> = self.instances.lock().keys().cloned().collect();
        let dead: Vec<String> = tab_ids.into_iter().filter(|id| !is_live(id)).collect();
        if dead.is_empty() {
            return Vec::new();
        }
        let mut instances = self.instances.lock();
        dead.iter()
            .filter_map(|id| instances.remove(id))
            .map(|instance| instance.runtime)
            .collect()
    }
}

#[cfg(test)]
mod tests;
