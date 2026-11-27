mod accessible_wait;
mod atomic;
mod blueprint_origins;
mod blueprint_read;
mod calls;
mod completion;
mod diagnostics;
mod dialog;
mod domios_instance;
mod drag;
mod grab;
mod heuristics;
mod hit_test;
mod inbox;
mod input;
mod interactions;
mod locators;
mod metrics;
mod mouse;
mod output;
mod overlay;
mod page;
mod redaction;
mod resume;
mod screenshot;
mod sessions;
mod snapshots;
mod snip;
mod stream;
mod text;
mod typing;
mod upload;
mod verbs;
mod viewport;

pub use blueprint_origins::allows_page_origin;
pub use blueprint_read::{NamedPageRead, NamedPageResult, PageCapture};

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::{BoxFuture, Shared};
use futures_util::FutureExt;
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
    AgentCloseMode, SurfaceCallCtx, SurfaceCallOutcome, SurfaceEvents, SurfaceNotice,
    SurfaceResume, SurfaceRuntime, SurfaceRuntimeFactory,
};
use codeterm_browser::process::CloseOutcome;
use codeterm_browser::profile::ProfileLayout;
use codeterm_browser::viewport::ViewportRequest;
use codeterm_browser::{BrowserHost, BrowserHostConfig, BrowserStatus, OpenOptions, PageChannel};

pub const BROWSER_RUNTIME: &str = "browser";
pub(super) const WAIT_DEADLINE: Duration = Duration::from_secs(15);
const PENDING_GRABS: usize = 16;
const CLEAN_PROFILE: &str = "clean";

type BrowserSweep = tokio::sync::OnceCell<Shared<BoxFuture<'static, ()>>>;

pub struct BrowserRuntimeFactory {
    data_dir: PathBuf,
    swept: Arc<BrowserSweep>,
}

impl BrowserRuntimeFactory {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
            swept: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    pub async fn sweep(&self) {
        sweep_browsers(&self.swept, ProfileLayout::under_data_dir(&self.data_dir)).await;
    }
}

async fn sweep_browsers(swept: &BrowserSweep, layout: ProfileLayout) {
    swept
        .get_or_init(|| {
            futures_util::future::ready(
                async move {
                    let report = codeterm_browser::sweep_orphans(layout).await;
                    log::debug!("[browser] startup orphan sweep: {report:?}");
                }
                .boxed()
                .shared(),
            )
        })
        .await
        .clone()
        .await;
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
pub struct BrowserRuntime {
    host: BrowserHost,
    viewport: tokio::sync::Mutex<viewport::ViewportOwner>,
    take_control: Mutex<HashMap<String, u64>>,
    layout: ProfileLayout,
    swept: Arc<BrowserSweep>,
    shared: Arc<PageShared>,
    launch_gate: tokio::sync::Mutex<()>,
    profile_request: Mutex<String>,
    resume_profile: tokio::sync::Mutex<Option<String>>,
    resume_url: Mutex<Option<String>>,
    grabs: Mutex<GrabStore>,
    snips: Mutex<SnipStore>,
    inbox: Mutex<AnnotationInbox>,
    secrets: Mutex<Vec<String>>,
    uploads: Mutex<Vec<upload::AdmittedFile>>,
    upload_gate: tokio::sync::Mutex<()>,
    action_gate: Arc<tokio::sync::Mutex<()>>,
    overlay_session: tokio::sync::Mutex<Option<String>>,
}

fn cdp_error(error: codeterm_browser::CdpError) -> String {
    error.to_string()
}

impl BrowserRuntime {
    fn new(
        config: BrowserHostConfig,
        owner: &str,
        owner_scope: &str,
        swept: Arc<BrowserSweep>,
        resume: SurfaceResume,
    ) -> Self {
        let resume = SurfaceResume {
            url: resume.url.as_deref().and_then(resume::resumable_url),
            profile: resume
                .profile
                .as_deref()
                .and_then(resume::resumable_profile),
        };
        log::debug!("[browser] owner scope {owner_scope}, default temporary session");
        let shared = PageShared::new_with_profile(owner, resume.profile.as_deref());
        shared.seed_resume(resume.clone());
        Self {
            layout: config.layout.clone(),
            host: BrowserHost::new(config),
            viewport: tokio::sync::Mutex::new(viewport::ViewportOwner::default()),
            take_control: Mutex::new(HashMap::new()),
            swept,
            shared,
            launch_gate: tokio::sync::Mutex::new(()),
            profile_request: Mutex::new(String::new()),
            resume_profile: tokio::sync::Mutex::new(resume.profile),
            resume_url: Mutex::new(resume.url),
            grabs: Mutex::new(GrabStore::default()),
            snips: Mutex::new(SnipStore::default()),
            inbox: Mutex::new(AnnotationInbox::default()),
            secrets: Mutex::new(Vec::new()),
            uploads: Mutex::new(Vec::new()),
            upload_gate: tokio::sync::Mutex::new(()),
            action_gate: Arc::new(tokio::sync::Mutex::new(())),
            overlay_session: tokio::sync::Mutex::new(None),
        }
    }

    /// Once, before the first launch: a remembered profile that was deleted falls back to clean.
    async fn apply_resume_profile(&self) {
        let mut pending = self.resume_profile.lock().await;
        let Some(wanted) = pending.take() else {
            return;
        };
        let layout = self.layout.clone();
        let wanted_lookup = wanted.clone();
        let resolved = tokio::task::spawn_blocking(move || {
            resume::restored_profile_in_layout(&layout, &wanted_lookup)
        })
        .await
        .ok()
        .flatten();
        match resolved {
            Some(name) => {
                *self.profile_request.lock() = name.clone();
                self.shared.update(|state| state.profile = name.clone());
                self.shared.remember_profile(&name);
            }
            None => {
                let fallback = self.profile_request.lock().clone();
                let label = if fallback.is_empty() {
                    CLEAN_PROFILE.to_string()
                } else {
                    fallback
                };
                self.shared.update(|state| state.profile = label.clone());
                self.shared.remember_profile(&label);
                self.shared.notice(SurfaceNotice::info(format!(
                    "Profile {wanted} no longer exists; reopened with a temporary session."
                )));
            }
        }
    }

    /// Once, after the first successful launch; a failed load keeps the tab on the error page.
    async fn apply_resume_url(&self, page: &PageChannel) {
        let Some(url) = self.resume_url.lock().take() else {
            return;
        };
        if let Err(error) = self
            .navigate_for_viewer(page, NavAction::Goto, Some(&url))
            .await
        {
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
        self.ensure_page_with_profile(None).await
    }

    async fn ensure_page_with_profile(
        &self,
        exclusive_profile: Option<&str>,
    ) -> Result<PageChannel, String> {
        let page = {
            let _launch = self.launch_gate.lock().await;
            self.viewport.lock().await.start(self.host.viewport());
            sweep_browsers(&self.swept, self.layout.clone()).await;
            self.apply_resume_profile().await;
            let viewport = self.host.viewport();
            let options = OpenOptions {
                profile: exclusive_profile
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.profile_request.lock().clone()),
                width: viewport.width as f64,
                height: viewport.height as f64,
                dpr: viewport.dpr,
            };
            let opened = match exclusive_profile {
                Some(_) => self.host.open_exclusive(options).await?,
                None => self.host.open(options).await?,
            };
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
            self.ensure_page_domains(&page).await?;
            page
        };
        self.apply_resume_url(&page).await;
        Ok(page)
    }

    /// Cancellation-safe domain setup: commits `domains_ready` only after all three acks succeed.
    pub(super) async fn ensure_page_domains(&self, page: &PageChannel) -> Result<(), String> {
        if !self.shared.needs_domains_setup(page) {
            return Ok(());
        }
        self.send(page, "Runtime.enable", json!({})).await?;
        self.install_overlay(page).await?;
        self.send(
            page,
            "Network.enable",
            json!({"maxTotalBufferSize":0,"maxResourceBufferSize":0,"maxPostDataSize":0}),
        )
        .await?;
        self.shared.mark_domains_ready(page);
        Ok(())
    }

    async fn send(&self, page: &PageChannel, method: &str, params: Value) -> Result<Value, String> {
        if method == "Page.captureScreenshot" {
            return self.capture_screenshot(page, params, false).await;
        }
        page.send(method, params).await.map_err(cdp_error)
    }

    async fn dispatch_input(&self, page: &PageChannel, input: &InputArgs) -> Result<(), String> {
        page.send_batch(input_commands(input)?)
            .await
            .map_err(cdp_error)?;
        Ok(())
    }

    /// Human-initiated toolbar navigation returns as soon as Chromium starts navigating so
    /// slow third-party resources never hold `action_gate` and freeze interactive input.
    async fn navigate_for_viewer(
        &self,
        page: &PageChannel,
        action: NavAction,
        target: Option<&str>,
    ) -> Result<(), String> {
        self.shared.invalidate_snapshot();
        self.shared.invalidate_action();
        let (method, params) = match action {
            NavAction::Reload => ("Page.reload", json!({})),
            NavAction::Goto => {
                let href = input::goto_href(target.unwrap_or(""))?;
                ("Page.navigate", json!({"url":href}))
            }
            NavAction::Back | NavAction::Forward => {
                let history = self
                    .send(page, "Page.getNavigationHistory", json!({}))
                    .await?;
                match input::history_entry(&history, action == NavAction::Back) {
                    Some(entry_id) => ("Page.navigateToHistoryEntry", json!({"entryId":entry_id})),
                    None => return Ok(()),
                }
            }
        };
        let reply = self.send(page, method, params).await?;
        if let Some(error) = reply["errorText"]
            .as_str()
            .filter(|error| !error.is_empty())
        {
            return Err(format!("navigation failed: {error}"));
        }
        Ok(())
    }

    async fn navigate(
        &self,
        page: &PageChannel,
        action: NavAction,
        target: Option<&str>,
    ) -> Result<String, String> {
        self.navigate_for_agent(page, action, target).await
    }

    pub(super) async fn screenshot_base64(
        &self,
        page: &PageChannel,
        full: bool,
        include_overlay: bool,
    ) -> Result<String, String> {
        let shot = self
            .capture_screenshot(
                page,
                json!({ "format": "png", "captureBeyondViewport": full }),
                include_overlay,
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
        json!(codeterm_browser::surface_contracts::ProfilesResult {
            current: state.profile,
            profiles: state.profiles
        })
    }

    async fn refresh_profiles(&self) {
        let profiles = self.host.profiles().await;
        self.shared.update(|state| state.profiles = profiles);
    }

    /// `""` or `"clean"` is the throwaway profile; a named one keeps logins across restarts.
    async fn use_profile(&self, raw: &str, check_dialog: bool) -> Result<(), String> {
        if check_dialog {
            self.shared.check_page_verb()?;
        }
        sweep_browsers(&self.swept, self.layout.clone()).await;
        let wanted = if raw.trim().is_empty() || raw.trim().eq_ignore_ascii_case(CLEAN_PROFILE) {
            String::new()
        } else {
            let layout = self.layout.clone();
            let requested = codeterm_browser::profile::validate_named_profile(raw)?.to_string();
            tokio::task::spawn_blocking(move || layout.resolve_named(&requested))
                .await
                .map_err(|error| error.to_string())??
        };
        let label = if wanted.is_empty() {
            "a clean profile".to_string()
        } else {
            format!("profile {wanted}")
        };
        let _launch = self.launch_gate.lock().await;
        if check_dialog {
            self.shared.check_page_verb()?;
        }
        self.shared
            .notice(SurfaceNotice::info(format!("Switching to {label}…")));
        self.cancel_resume().await;
        let opened = self.host.use_profile(&wanted).await?;
        *self.profile_request.lock() = wanted;
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

    fn remember_secret(&self, secret: &str) -> Result<(), String> {
        if secret.is_empty() {
            return Err("empty_browser_secret".into());
        }
        let mut secrets = self.secrets.lock();
        if !secrets.iter().any(|known| known == secret) {
            if secrets.len() >= 128 {
                return Err("Browser secret limit reached; reopen the Browser".into());
            }
            secrets.push(secret.to_string());
        }
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

    pub(super) fn claim_control(&self, viewer: &str, generation: u64) {
        self.take_control
            .lock()
            .insert(viewer.to_string(), generation);
    }

    pub(super) fn release_control(&self, viewer: &str) {
        self.take_control.lock().remove(viewer);
    }

    pub(super) fn release_control_if_generation(&self, viewer: &str, generation: u64) {
        let mut holders = self.take_control.lock();
        if holders.get(viewer).copied() == Some(generation) {
            holders.remove(viewer);
        }
    }

    pub(super) fn owner_holds_control(&self) -> bool {
        !self.take_control.lock().is_empty()
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
    fn view_only_methods(&self) -> &[&str] {
        &[
            "profiles.create",
            "profiles.delete",
            "viewport.claim",
            "control.claim",
            "control.release",
        ]
    }

    fn set_owner_name(&self, name: &str) {
        self.shared.update(|state| state.owner = name.to_string());
    }

    #[cfg(test)]
    fn state(&self) -> SurfaceState {
        self.shared.state()
    }

    async fn connect(
        self: Arc<Self>,
        viewport: Option<ViewportRequest>,
        viewer: Option<String>,
        viewer_target: Option<String>,
        interactive: bool,
    ) -> SurfaceEvents {
        stream::connect(self, viewport, viewer, viewer_target, interactive).await
    }

    async fn call(
        &self,
        ctx: &SurfaceCallCtx,
        method: &str,
        args: Value,
    ) -> Result<SurfaceCallOutcome, String> {
        self.view_call(ctx, method, args)
            .await
            .map(|outcome| self.redact_outcome(outcome))
            .map_err(|error| self.redact_text(&error))
    }

    async fn agent_verb(
        &self,
        ctx: &SurfaceCallCtx,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        if matches!(verb, "type-secret" | "connect-secret") {
            let secret = args
                .last()
                .ok_or("Secret typing needs a target and stdin text")?;
            if secret.is_empty() || args.len() < 2 {
                return Err("Secret typing needs a target and nonempty stdin text".into());
            }
            self.remember_secret(secret)?;
        }
        self.run_verb(ctx, verb, args)
            .await
            .map(|result| self.redact_text(&result))
            .map_err(|error| {
                let error = if (error.starts_with("stale:") || error.contains("re-snapshot"))
                    && !error.contains("domios plugin browser snapshot")
                {
                    format!("{error}; run `domios plugin browser snapshot` for fresh refs")
                } else {
                    error
                };
                self.redact_text(&error)
            })
    }

    async fn annotate(
        &self,
        ctx: &SurfaceCallCtx,
        args: &[String],
    ) -> Result<SurfaceCallOutcome, String> {
        self.run_annotate(ctx, args)
            .await
            .map(|outcome| self.redact_outcome(outcome))
            .map_err(|error| self.redact_text(&error))
    }

    fn annotation_delivered(&self, id: &str) {
        self.inbox.lock().mark_delivered(id);
    }

    fn resume_updates(&self) -> Option<tokio::sync::watch::Receiver<SurfaceResume>> {
        Some(self.shared.subscribe_resume())
    }

    fn browser_status(&self) -> BrowserStatus {
        match self.host.status() {
            BrowserStatus::Error(reason) => BrowserStatus::Error(self.redact_text(&reason)),
            status => status,
        }
    }

    fn retire(&self) {
        self.host.retire();
    }

    async fn dispose(&self) {
        self.shared.unbind();
        if let Err(error) =
            codeterm_browser::process::termination::close_result(self.host.shutdown().await)
        {
            log::warn!("[browser] surface disposal incomplete: {error}");
        }
    }

    async fn close(&self) -> Result<(), super::model::SurfaceError> {
        self.shared.unbind();
        codeterm_browser::process::termination::close_result(self.host.close().await)
            .map_err(super::model::SurfaceError::TerminationFailed)
    }

    async fn agent_close(
        &self,
        ctx: &SurfaceCallCtx,
        mode: AgentCloseMode,
    ) -> Result<Option<String>, super::model::SurfaceError> {
        match mode {
            AgentCloseMode::Normal => {
                self.shared
                    .check_page_verb()
                    .map_err(super::model::SurfaceError::Blocked)?;
                let _launch = self.launch_gate.lock().await;
                self.shared
                    .check_page_verb()
                    .map_err(super::model::SurfaceError::Blocked)?;
                self.close().await?;
                Ok(None)
            }
            AgentCloseMode::DiscardUnsaved => self.discard_unsaved_session(ctx).await,
        }
    }

    async fn shutdown(&self, graceful: Duration) -> bool {
        self.shared.unbind();
        !matches!(
            self.host.shutdown_within(graceful).await,
            Some(CloseOutcome::Killed(_))
        )
    }
}

#[cfg(test)]
mod profile_policy_tests;
