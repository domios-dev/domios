mod calls;
mod grab;
mod heuristics;
mod inbox;
mod input;
mod page;
mod resume;
mod snip;
mod stream;
mod verbs;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use self::grab::GrabTarget;
use self::inbox::AnnotationInbox;
use self::input::{input_commands, InputArgs, NavAction};
use self::page::PageShared;
use self::snip::SnipStore;
#[cfg(test)]
use super::runtime::SurfaceState;
use super::runtime::{
    SurfaceCallCtx, SurfaceCallOutcome, SurfaceEvents, SurfaceNotice, SurfaceResume,
    SurfaceRuntime, SurfaceRuntimeFactory,
};
use crate::service::browser::process::CloseOutcome;
use crate::service::browser::profile::{owner_profile, sanitize_profile_name, ProfileLayout};
use crate::service::browser::viewport::ViewportRequest;
use crate::service::browser::{BrowserHost, BrowserHostConfig, OpenOptions, PageChannel};

pub(crate) const BROWSER_RUNTIME: &str = "browser";
pub(super) const WAIT_DEADLINE: Duration = Duration::from_secs(15);
const PENDING_GRABS: usize = 16;
const CLEAN_PROFILE: &str = "clean";

pub(crate) struct BrowserRuntimeFactory {
    data_dir: PathBuf,
    swept: Arc<tokio::sync::OnceCell<()>>,
}

impl BrowserRuntimeFactory {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            swept: Arc::new(tokio::sync::OnceCell::new()),
        }
    }
}

impl SurfaceRuntimeFactory for BrowserRuntimeFactory {
    fn create(
        &self,
        owner_name: &str,
        owner_scope: &str,
        resume: Option<SurfaceResume>,
    ) -> Arc<dyn SurfaceRuntime> {
        Arc::new(BrowserRuntime::new(
            BrowserHostConfig::under_data_dir(&self.data_dir),
            owner_name,
            owner_scope,
            self.swept.clone(),
            resume.unwrap_or_default(),
        ))
    }
}

#[derive(Default)]
struct GrabStore {
    seq: u64,
    pending: VecDeque<(String, GrabTarget, Option<String>)>,
}

/// The browser surface runtime: one `BrowserHost`, its page listener, grabs and annotation inbox.
pub(crate) struct BrowserRuntime {
    host: BrowserHost,
    layout: ProfileLayout,
    swept: Arc<tokio::sync::OnceCell<()>>,
    shared: Arc<PageShared>,
    profile_request: Mutex<String>,
    resume_profile: tokio::sync::Mutex<Option<String>>,
    resume_url: Mutex<Option<String>>,
    grabs: Mutex<GrabStore>,
    snips: Mutex<SnipStore>,
    inbox: Mutex<AnnotationInbox>,
}

fn cdp_error(error: crate::service::browser::CdpError) -> String {
    error.to_string()
}

impl BrowserRuntime {
    fn new(
        config: BrowserHostConfig,
        owner: &str,
        owner_scope: &str,
        swept: Arc<tokio::sync::OnceCell<()>>,
        resume: SurfaceResume,
    ) -> Self {
        let resume = SurfaceResume {
            url: resume.url.as_deref().and_then(resume::resumable_url),
            profile: resume
                .profile
                .as_deref()
                .and_then(resume::resumable_profile),
        };
        log::debug!(
            "[browser] owner scope {owner_scope}, default profile {}",
            owner_profile(owner_scope)
        );
        let shared = PageShared::new(owner);
        shared.seed_resume(resume.clone());
        Self {
            layout: config.layout.clone(),
            host: BrowserHost::new(config),
            swept,
            shared,
            profile_request: Mutex::new(owner_profile(owner_scope)),
            resume_profile: tokio::sync::Mutex::new(resume.profile),
            resume_url: Mutex::new(resume.url),
            grabs: Mutex::new(GrabStore::default()),
            snips: Mutex::new(SnipStore::default()),
            inbox: Mutex::new(AnnotationInbox::default()),
        }
    }

    /// Once, before the first launch: a remembered profile that was deleted falls back to clean.
    async fn apply_resume_profile(&self) {
        let mut pending = self.resume_profile.lock().await;
        let Some(wanted) = pending.take() else {
            return;
        };
        let available = self.host.profiles().await;
        match resume::restored_profile(&wanted, &available) {
            Some(name) => *self.profile_request.lock() = name,
            None => {
                self.shared.remember_profile(&self.profile_request.lock());
                self.shared.notice(SurfaceNotice::info(format!(
                    "Profile {wanted} no longer exists; reopened with the owner profile."
                )));
            }
        }
    }

    /// Once, after the first successful launch; a failed load keeps the tab on the error page.
    async fn apply_resume_url(&self, page: &PageChannel) {
        let Some(url) = self.resume_url.lock().take() else {
            return;
        };
        if let Err(error) = self.navigate(page, NavAction::Goto, Some(&url)).await {
            self.shared.notice(SurfaceNotice::error(format!(
                "Could not reopen the last page: {error}"
            )));
        }
    }

    /// An explicit profile choice wins over the remembered page and profile.
    async fn cancel_resume(&self) {
        self.resume_profile.lock().await.take();
        self.resume_url.lock().take();
    }

    /// Starts (or reuses) Chromium under the requested profile and binds its page listener.
    async fn ensure_page(&self) -> Result<PageChannel, String> {
        let layout = self.layout.clone();
        self.swept
            .get_or_init(|| async move {
                let report = crate::service::browser::sweep_orphans(layout).await;
                log::info!("[browser] orphan sweep before first launch: {report:?}");
            })
            .await;
        self.apply_resume_profile().await;
        let profile = self.profile_request.lock().clone();
        let viewport = self.host.viewport();
        let opened = self
            .host
            .open(OpenOptions {
                profile,
                width: viewport.width as f64,
                height: viewport.height as f64,
                dpr: viewport.dpr,
            })
            .await?;
        let page = self.host.page().ok_or("the browser has no page")?;
        if !opened.reused {
            self.shared.unbind();
            let profiles = self.host.profiles().await;
            self.shared.update(|state| {
                state.profile = opened.profile.clone();
                state.profiles = profiles;
            });
            self.shared.remember_profile(&opened.profile);
        }
        self.shared.bind(&page);
        self.apply_resume_url(&page).await;
        Ok(page)
    }

    async fn send(&self, page: &PageChannel, method: &str, params: Value) -> Result<Value, String> {
        page.send(method, params).await.map_err(cdp_error)
    }

    async fn dispatch_input(&self, page: &PageChannel, input: &InputArgs) -> Result<(), String> {
        page.send_batch(input_commands(input)?)
            .await
            .map_err(cdp_error)?;
        Ok(())
    }

    async fn navigate(
        &self,
        page: &PageChannel,
        action: NavAction,
        target: Option<&str>,
    ) -> Result<(), String> {
        match action {
            NavAction::Reload => self.send(page, "Page.reload", json!({})).await.map(|_| ()),
            NavAction::Goto => {
                let href = input::goto_href(target.unwrap_or(""))?;
                let result = self
                    .send(page, "Page.navigate", json!({ "url": href }))
                    .await?;
                match result.get("errorText").and_then(Value::as_str) {
                    Some(error) if !error.is_empty() => Err(format!("navigation failed: {error}")),
                    _ => Ok(()),
                }
            }
            NavAction::Back | NavAction::Forward => {
                let history = self
                    .send(page, "Page.getNavigationHistory", json!({}))
                    .await?;
                match input::history_entry(&history, action == NavAction::Back) {
                    Some(entry_id) => self
                        .send(
                            page,
                            "Page.navigateToHistoryEntry",
                            json!({ "entryId": entry_id }),
                        )
                        .await
                        .map(|_| ()),
                    None => Ok(()),
                }
            }
        }
    }

    async fn screenshot_base64(&self, page: &PageChannel, full: bool) -> Result<String, String> {
        let shot = self
            .send(
                page,
                "Page.captureScreenshot",
                json!({ "format": "png", "captureBeyondViewport": full }),
            )
            .await?;
        shot.get("data")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "the screenshot returned no image".to_string())
    }

    /// Writes off the async runtime into the owner's pad, or the data dir without one.
    async fn save_evidence(
        &self,
        ctx: &SurfaceCallCtx,
        name: String,
        base64: String,
    ) -> Result<PathBuf, String> {
        let ctx = ctx.clone();
        tokio::task::spawn_blocking(move || {
            inbox::write_evidence(&ctx.evidence_dir(), &name, &base64)
        })
        .await
        .map_err(|error| format!("screenshot writer failed: {error}"))?
    }

    fn profile_view(&self) -> Value {
        let state = self.shared.state();
        json!({ "current": state.profile, "profiles": state.profiles })
    }

    async fn refresh_profiles(&self) {
        let profiles = self.host.profiles().await;
        self.shared.update(|state| state.profiles = profiles);
    }

    /// `""` or `"clean"` is the throwaway profile; a named one keeps logins across restarts.
    async fn use_profile(&self, raw: &str) -> Result<(), String> {
        let wanted = if raw.trim().eq_ignore_ascii_case(CLEAN_PROFILE) {
            String::new()
        } else {
            sanitize_profile_name(raw)
        };
        let label = if wanted.is_empty() {
            "a clean profile".to_string()
        } else {
            format!("profile {wanted}")
        };
        self.shared
            .notice(SurfaceNotice::info(format!("Switching to {label}…")));
        self.cancel_resume().await;
        *self.profile_request.lock() = wanted.clone();
        let opened = self.host.use_profile(&wanted).await?;
        if !opened.reused {
            self.shared.unbind();
        }
        if let Some(page) = self.host.page() {
            self.shared.bind(&page);
        }
        let profiles = self.host.profiles().await;
        self.shared.update(|state| {
            state.profile = opened.profile.clone();
            state.profiles = profiles;
        });
        self.shared.remember_profile(&opened.profile);
        Ok(())
    }

    fn next_grab_id(&self) -> String {
        let mut grabs = self.grabs.lock();
        grabs.seq += 1;
        format!("g{}", grabs.seq)
    }

    fn keep_grab(&self, id: &str, target: GrabTarget, png: Option<String>) {
        let mut grabs = self.grabs.lock();
        grabs.pending.push_back((id.to_string(), target, png));
        while grabs.pending.len() > PENDING_GRABS {
            grabs.pending.pop_front();
        }
    }

    fn take_grab(&self, id: &str) -> Option<(GrabTarget, Option<String>)> {
        let mut grabs = self.grabs.lock();
        let index = grabs
            .pending
            .iter()
            .position(|(pending, ..)| pending == id)?;
        grabs
            .pending
            .remove(index)
            .map(|(_, target, png)| (target, png))
    }
}

#[async_trait::async_trait]
impl SurfaceRuntime for BrowserRuntime {
    fn set_owner_name(&self, name: &str) {
        self.shared.update(|state| state.owner = name.to_string());
    }

    #[cfg(test)]
    fn state(&self) -> SurfaceState {
        self.shared.state()
    }

    async fn connect(self: Arc<Self>, viewport: Option<ViewportRequest>) -> SurfaceEvents {
        stream::connect(self, viewport).await
    }

    async fn call(
        &self,
        ctx: &SurfaceCallCtx,
        method: &str,
        args: Value,
    ) -> Result<SurfaceCallOutcome, String> {
        self.view_call(ctx, method, args).await
    }

    async fn agent_verb(
        &self,
        ctx: &SurfaceCallCtx,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        self.run_verb(ctx, verb, args).await
    }

    async fn annotate(
        &self,
        ctx: &SurfaceCallCtx,
        args: &[String],
    ) -> Result<SurfaceCallOutcome, String> {
        self.run_annotate(ctx, args).await
    }

    fn annotation_delivered(&self, id: &str) {
        self.inbox.lock().mark_delivered(id);
    }

    fn resume_updates(&self) -> Option<tokio::sync::watch::Receiver<SurfaceResume>> {
        Some(self.shared.subscribe_resume())
    }

    async fn dispose(&self) {
        self.shared.unbind();
        self.host.shutdown().await;
    }

    async fn shutdown(&self, graceful: Duration) -> bool {
        self.shared.unbind();
        !matches!(
            self.host.shutdown_within(graceful).await,
            Some(CloseOutcome::Killed(_))
        )
    }
}
