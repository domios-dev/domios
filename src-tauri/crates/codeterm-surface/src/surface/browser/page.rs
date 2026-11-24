use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{broadcast, watch};

use super::dialog::{ActiveDialog, DialogState};
use super::heuristics::{needs_human, IDLE, NEEDS_NONE};
use super::resume::{resumable_profile, resumable_url};
use crate::surface::runtime::{SurfaceNotice, SurfaceResume, SurfaceState};
use codeterm_browser::cdp_wire::CdpEvent;
use codeterm_browser::snapshot::{resolve_target, ResolvedTarget, Snapshot};
use codeterm_browser::{EventStream, PageChannel};

const ATTACHED_PROFILE: &str = "attached";
pub(super) const CURSOR_BINDING: &str = "__domiosCursor";

pub(super) const PAGE_EVENTS: &[&str] = &[
    "Runtime.consoleAPICalled",
    "Runtime.bindingCalled",
    "Network.requestWillBeSent",
    "Network.responseReceived",
    "Page.frameNavigated",
    "Page.navigatedWithinDocument",
    "Target.targetInfoChanged",
    "Page.javascriptDialogOpening",
    "Page.javascriptDialogClosed",
];

pub(super) fn sanitize_cursor(raw: &str) -> &'static str {
    match raw.trim() {
        "auto" => "auto",
        "default" => "default",
        "none" => "none",
        "context-menu" => "context-menu",
        "help" => "help",
        "pointer" => "pointer",
        "progress" => "progress",
        "wait" => "wait",
        "cell" => "cell",
        "crosshair" => "crosshair",
        "text" => "text",
        "vertical-text" => "vertical-text",
        "alias" => "alias",
        "copy" => "copy",
        "move" => "move",
        "no-drop" => "no-drop",
        "not-allowed" => "not-allowed",
        "grab" => "grab",
        "grabbing" => "grabbing",
        "all-scroll" => "all-scroll",
        "col-resize" => "col-resize",
        "row-resize" => "row-resize",
        "n-resize" => "n-resize",
        "e-resize" => "e-resize",
        "s-resize" => "s-resize",
        "w-resize" => "w-resize",
        "ne-resize" => "ne-resize",
        "nw-resize" => "nw-resize",
        "se-resize" => "se-resize",
        "sw-resize" => "sw-resize",
        "ew-resize" => "ew-resize",
        "ns-resize" => "ns-resize",
        "nesw-resize" => "nesw-resize",
        "nwse-resize" => "nwse-resize",
        "zoom-in" => "zoom-in",
        "zoom-out" => "zoom-out",
        _ => "default",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PageUpdate {
    Navigated { url: String },
    SameDocument { url: String },
    TargetInfo { title: String, url: String },
    DialogOpened(ActiveDialog),
    DialogClosed,
}

/// What one CDP event means for the surface state of the page `target_id`.
pub(super) fn page_update(method: &str, params: &Value, target_id: &str) -> Option<PageUpdate> {
    let text = |value: Option<&Value>| value.and_then(Value::as_str).unwrap_or("").to_string();
    match method {
        "Page.frameNavigated" => {
            let frame = params.get("frame")?;
            frame
                .get("parentId")
                .is_none_or(Value::is_null)
                // CDP reports the fragment apart from the document URL.
                .then(|| PageUpdate::Navigated {
                    url: format!(
                        "{}{}",
                        text(frame.get("url")),
                        text(frame.get("urlFragment"))
                    ),
                })
        }
        // A page target's main frame id is its target id.
        "Page.navigatedWithinDocument" => (params.get("frameId").and_then(Value::as_str)
            == Some(target_id))
        .then(|| PageUpdate::SameDocument {
            url: text(params.get("url")),
        }),
        "Target.targetInfoChanged" => {
            let info = params.get("targetInfo")?;
            let ours = info.get("targetId").and_then(Value::as_str) == Some(target_id)
                && info.get("type").and_then(Value::as_str) == Some("page");
            ours.then(|| PageUpdate::TargetInfo {
                title: text(info.get("title")),
                url: text(info.get("url")),
            })
        }
        "Page.javascriptDialogOpening" => Some(PageUpdate::DialogOpened(
            ActiveDialog::from_cdp_params(params),
        )),
        "Page.javascriptDialogClosed" => Some(PageUpdate::DialogClosed),
        _ => None,
    }
}

/// Versioned dialog watch state so stale snapshots published after releasing `session` never
/// overwrite a newer dialog opening or blocking transition.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DialogSnapshot {
    pub seq: u64,
    pub dialog: Option<ActiveDialog>,
}

#[derive(Default)]
struct PageSession {
    cursor_visible: bool,
    diagnostics: std::collections::VecDeque<Value>,
    action: Option<String>,
    bound_page: Option<PageChannel>,
    bound_session: Option<String>,
    snapshot: Option<Snapshot>,
    capture: Option<String>,
    dialog: DialogState,
    dialog_seq: u64,
    domains_ready: bool,
    grab_ready: bool,
    listener: Option<tokio::task::AbortHandle>,
}

fn next_dialog_snapshot(session: &mut PageSession) -> DialogSnapshot {
    session.dialog_seq += 1;
    DialogSnapshot {
        seq: session.dialog_seq,
        dialog: session.dialog.active().cloned(),
    }
}

/// State a page listener and the runtime both write; one per browser surface instance.
pub(super) struct PageShared {
    state: watch::Sender<SurfaceState>,
    notices: broadcast::Sender<SurfaceNotice>,
    resume: watch::Sender<SurfaceResume>,
    cursor: watch::Sender<String>,
    dialogs: watch::Sender<DialogSnapshot>,
    session: Mutex<PageSession>,
}

impl PageShared {
    pub(super) fn show_cursor(&self, page: &PageChannel) {
        let mut session = self.session.lock();
        if session
            .bound_page
            .as_ref()
            .is_some_and(|bound| bound.same_page(page))
        {
            session.cursor_visible = true;
        }
    }

    pub(super) fn take_cursor(&self, page: &PageChannel) -> bool {
        let mut session = self.session.lock();
        session
            .bound_page
            .as_ref()
            .is_some_and(|bound| bound.same_page(page))
            && std::mem::take(&mut session.cursor_visible)
    }

    pub(super) fn diagnostics(&self, console: bool, pattern: Option<&str>, limit: usize) -> Value {
        let session = self.session.lock();
        let kind = if console { "console" } else { "network" };
        let mut entries: Vec<_> = session
            .diagnostics
            .iter()
            .rev()
            .filter(|entry| {
                entry["kind"] == kind
                    && pattern.is_none_or(|p| entry["text"].as_str().is_some_and(|s| s.contains(p)))
            })
            .take(limit)
            .cloned()
            .collect();
        entries.reverse();
        serde_json::json!({"entries":entries,"limit":limit,"retainedLimit":200})
    }

    pub(super) fn begin_action(&self, page: &PageChannel) -> Result<String, String> {
        let mut session = self.session.lock();
        if !session
            .bound_page
            .as_ref()
            .is_some_and(|bound| bound.same_page(page))
        {
            return Err("stale: browser session changed".into());
        }
        let token = uuid::Uuid::new_v4().to_string();
        session.action = Some(token.clone());
        Ok(token)
    }

    pub(super) fn check_action(&self, token: &str) -> Result<(), String> {
        if self.session.lock().action.as_deref() != Some(token) {
            return Err("action_unconfirmed: browser authority changed; inspect before continuing; never replay".into());
        }
        Ok(())
    }

    pub(super) fn publish_dialog_snapshot(&self, snapshot: DialogSnapshot) {
        self.dialogs.send_if_modified(|current| {
            if snapshot.seq <= current.seq {
                return false;
            }
            *current = snapshot;
            true
        });
        self.update(|state| {
            let blocking = self.dialogs.borrow().dialog.is_some();
            state.needs_human = needs_human(&state.url, &state.title, blocking).to_string();
        });
    }

    pub(super) fn invalidate_action(&self) {
        self.session.lock().action = None;
    }
    pub(super) fn new(owner: &str) -> Arc<Self> {
        Self::new_with_profile(owner, None)
    }

    pub(super) fn new_with_profile(owner: &str, profile: Option<&str>) -> Arc<Self> {
        let state = SurfaceState {
            owner: owner.to_string(),
            activity: IDLE.into(),
            needs_human: NEEDS_NONE.into(),
            profile: profile
                .and_then(resumable_profile)
                .unwrap_or_else(|| "clean".into()),
            ..Default::default()
        };
        Arc::new(Self {
            state: watch::channel(state).0,
            notices: broadcast::channel(16).0,
            resume: watch::channel(SurfaceResume::default()).0,
            cursor: watch::channel("default".to_string()).0,
            dialogs: watch::channel(DialogSnapshot::default()).0,
            session: Mutex::new(PageSession::default()),
        })
    }

    pub(super) fn subscribe_cursor(&self) -> watch::Receiver<String> {
        self.cursor.subscribe()
    }

    pub(super) fn set_cursor(&self, raw: &str) {
        let next = sanitize_cursor(raw);
        self.cursor.send_if_modified(|current| {
            if current == next {
                return false;
            }
            current.clear();
            current.push_str(next);
            true
        });
    }

    pub(super) fn seed_resume(&self, resume: SurfaceResume) {
        self.resume.send_replace(resume);
    }

    pub(super) fn subscribe_resume(&self) -> watch::Receiver<SurfaceResume> {
        self.resume.subscribe()
    }

    /// Committed top-level pages only; anything not worth reopening keeps the previous one.
    fn remember_url(&self, url: &str) {
        if self.state.borrow().profile == ATTACHED_PROFILE {
            return;
        }
        let Some(url) = resumable_url(url) else {
            return;
        };
        self.resume.send_if_modified(|resume| {
            let changed = resume.url.as_deref() != Some(url.as_str());
            resume.url = Some(url);
            changed
        });
    }

    pub(super) fn remember_profile(&self, label: &str) {
        if label == ATTACHED_PROFILE {
            return;
        }
        let profile = resumable_profile(label);
        self.resume.send_if_modified(|resume| {
            let changed = resume.profile != profile;
            resume.profile = profile;
            changed
        });
    }

    pub fn state(&self) -> SurfaceState {
        self.state.borrow().clone()
    }

    pub(super) fn subscribe_state(&self) -> watch::Receiver<SurfaceState> {
        self.state.subscribe()
    }

    pub(super) fn subscribe_notices(&self) -> broadcast::Receiver<SurfaceNotice> {
        self.notices.subscribe()
    }

    pub fn update(&self, change: impl FnOnce(&mut SurfaceState)) {
        self.state.send_if_modified(|state| {
            let before = state.clone();
            change(state);
            *state != before
        });
    }

    pub(super) fn notice(&self, notice: SurfaceNotice) {
        let _ = self.notices.send(notice);
    }

    /// `actor` names a non-owner agent driving this activity; it clears with the activity.
    pub(super) fn set_activity(&self, activity: &str, actor: Option<&str>) {
        let actor = actor
            .and_then(crate::surface::model::sanitize_actor)
            .unwrap_or_default();
        self.update(|state| {
            state.activity = activity.to_string();
            state.actor = actor;
        });
    }

    /// Binds the listener to `page` once per CDP session; a relaunch brings a new session.
    pub(super) fn bind(self: &Arc<Self>, page: &PageChannel) {
        let (events, snapshot) = {
            let mut session = self.session.lock();
            if session
                .bound_page
                .as_ref()
                .is_some_and(|bound| bound.same_page(page))
            {
                return;
            }
            if let Some(listener) = session.listener.take() {
                listener.abort();
            }
            let dialog_seq = session.dialog_seq;
            *session = PageSession {
                bound_page: Some(page.clone()),
                bound_session: Some(page.session_id().to_string()),
                dialog_seq,
                ..Default::default()
            };
            let snapshot = next_dialog_snapshot(&mut session);
            (page.subscribe(PAGE_EVENTS), snapshot)
        };
        self.publish_dialog_snapshot(snapshot);
        self.update(|state| {
            state.target_id = page.target_id().to_string();
        });
        let listener = tokio::spawn(listen(
            self.clone(),
            events,
            page.session_id().to_string(),
            page.target_id().to_string(),
        ))
        .abort_handle();
        let mut session = self.session.lock();
        if session.bound_session.as_deref() == Some(page.session_id()) {
            session.listener = Some(listener);
        } else {
            listener.abort();
        }
    }

    pub(super) fn unbind(&self) {
        let snapshot = {
            let mut session = self.session.lock();
            if let Some(listener) = session.listener.take() {
                listener.abort();
            }
            let dialog_seq = session.dialog_seq;
            *session = PageSession {
                dialog_seq,
                ..Default::default()
            };
            next_dialog_snapshot(&mut session)
        };
        self.publish_dialog_snapshot(snapshot);
        self.update(|state| {
            state.target_id.clear();
        });
    }

    pub(super) fn active_dialog(&self) -> Option<ActiveDialog> {
        self.session.lock().dialog.active().cloned()
    }

    pub(super) fn subscribe_dialogs(&self) -> watch::Receiver<DialogSnapshot> {
        self.dialogs.subscribe()
    }

    pub(super) fn check_page_verb(&self) -> Result<(), String> {
        self.session.lock().dialog.check_page_verb()
    }

    #[cfg(test)]
    pub(super) fn inject_dialog_for_test(&self, session_id: &str, dialog: ActiveDialog) {
        self.apply(session_id, PageUpdate::DialogOpened(dialog));
    }

    pub(super) fn is_bound_page(&self, page: &PageChannel) -> bool {
        self.session
            .lock()
            .bound_page
            .as_ref()
            .is_some_and(|bound| bound.same_page(page))
    }

    /// True until Runtime/overlay/Network setup has completed and been committed for `page`.
    pub(super) fn needs_domains_setup(&self, page: &PageChannel) -> bool {
        let session = self.session.lock();
        session
            .bound_page
            .as_ref()
            .is_some_and(|bound| bound.same_page(page))
            && !session.domains_ready
    }

    /// Commits domain setup only after every enable command has succeeded for `page`.
    pub(super) fn mark_domains_ready(&self, page: &PageChannel) {
        let mut session = self.session.lock();
        if session
            .bound_page
            .as_ref()
            .is_some_and(|bound| bound.same_page(page))
        {
            session.domains_ready = true;
        }
    }

    fn apply(&self, session_id: &str, update: PageUpdate) {
        let snapshot = {
            let mut session = self.session.lock();
            if session.bound_session.as_deref() != Some(session_id) {
                return;
            }
            let mut dialog_changed = false;
            match &update {
                PageUpdate::Navigated { .. } | PageUpdate::SameDocument { .. } => {
                    session.snapshot = None;
                    session.capture = None;
                    if matches!(update, PageUpdate::Navigated { .. }) {
                        session.cursor_visible = false;
                        let before = session.dialog.active().cloned();
                        session.dialog.on_navigated();
                        dialog_changed = session.dialog.active() != before.as_ref();
                    }
                }
                PageUpdate::DialogOpened(opened) => {
                    session.dialog.on_dialog_opening(opened.clone());
                    dialog_changed = true;
                }
                PageUpdate::DialogClosed => {
                    session.dialog.on_dialog_closed();
                    dialog_changed = true;
                }
                PageUpdate::TargetInfo { .. } => {}
            }
            dialog_changed.then(|| next_dialog_snapshot(&mut session))
        };
        if let Some(snapshot) = snapshot {
            self.publish_dialog_snapshot(snapshot);
        }
        if matches!(&update, PageUpdate::Navigated { .. }) {
            self.set_cursor("default");
        }
        if let PageUpdate::Navigated { url } | PageUpdate::SameDocument { url } = &update {
            self.remember_url(url);
        }
        self.update(|state| {
            match update {
                PageUpdate::Navigated { url } | PageUpdate::SameDocument { url } => state.url = url,
                PageUpdate::TargetInfo { title, url } => {
                    state.title = title;
                    state.url = url;
                }
                PageUpdate::DialogOpened(_) | PageUpdate::DialogClosed => {}
            }
            let blocking = self.dialogs.borrow().dialog.is_some();
            state.needs_human = needs_human(&state.url, &state.title, blocking).to_string();
        });
    }

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        self.session.lock().snapshot.clone()
    }

    pub(super) fn resolve(&self, locator: &[&str]) -> Result<ResolvedTarget, String> {
        let session = self.session.lock();
        resolve_target(session.snapshot.as_ref(), locator).map_err(|error| error.to_string())
    }

    pub(super) fn invalidate_snapshot(&self) {
        let mut session = self.session.lock();
        session.snapshot = None;
        session.capture = None;
    }

    pub(super) fn begin_snapshot(&self, session_id: &str) -> Result<String, String> {
        let mut session = self.session.lock();
        if session.bound_session.as_deref() != Some(session_id) {
            return Err(
                "the browser session changed — run `domios plugin browser snapshot` for fresh refs"
                    .into(),
            );
        }
        use base64::Engine;
        let capture = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(uuid::Uuid::new_v4().as_bytes());
        session.capture = Some(capture.clone());
        session.snapshot = None;
        Ok(capture)
    }

    pub(super) fn finish_snapshot(&self, capture: &str, snapshot: Snapshot) -> Result<(), String> {
        let mut session = self.session.lock();
        if session.capture.as_deref() != Some(capture) {
            return Err("the page changed during capture — run `domios plugin browser snapshot` for fresh refs".into());
        }
        session.snapshot = Some(snapshot);
        session.capture = None;
        Ok(())
    }

    /// True the first time per page session, so DOM/Overlay domains are enabled once.
    pub(super) fn claim_grab_setup(&self) -> bool {
        let mut session = self.session.lock();
        !std::mem::replace(&mut session.grab_ready, true)
    }

    pub(super) fn grab_ready(&self) -> bool {
        self.session.lock().grab_ready
    }

    pub(super) fn reset_grab_setup(&self) {
        self.session.lock().grab_ready = false;
    }
}

fn process_event(shared: &PageShared, session_id: &str, target_id: &str, event: &CdpEvent) {
    if event.session_id.as_deref() == Some(session_id) {
        if event.method == "Runtime.bindingCalled"
            && event.params.get("name").and_then(Value::as_str) == Some(CURSOR_BINDING)
        {
            if let Some(payload) = event.params.get("payload").and_then(Value::as_str) {
                shared.set_cursor(payload);
            }
            return;
        }
        if let Some(entry) = super::diagnostics::record(&event.method, &event.params) {
            let mut session = shared.session.lock();
            if session.bound_session.as_deref() == Some(session_id) {
                session.diagnostics.push_back(entry);
                while session.diagnostics.len() > 200 {
                    session.diagnostics.pop_front();
                }
            }
        }
    }
    if let Some(update) = page_update(&event.method, &event.params, target_id) {
        shared.apply(session_id, update);
    }
}

async fn listen(
    shared: Arc<PageShared>,
    mut events: EventStream,
    session_id: String,
    target_id: String,
) {
    while let Some(event) = events.recv().await {
        process_event(&shared, &session_id, &target_id, &event);
        while let Ok(queued) = events.try_recv() {
            process_event(&shared, &session_id, &target_id, &queued);
        }
    }
    let still_bound = shared.session.lock().bound_session.as_deref() == Some(session_id.as_str());
    if still_bound {
        shared.notice(SurfaceNotice::error(
            "The browser connection stopped. Reopen this surface to continue.",
        ));
        shared.update(|state| {
            state.activity = IDLE.into();
            state.target_id.clear();
        });
    }
}

#[cfg(test)]
mod tests;
