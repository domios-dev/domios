use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{broadcast, watch};

use super::heuristics::{needs_human, IDLE, NEEDS_NONE};
use super::resume::{resumable_profile, resumable_url};
use crate::service::browser::cdp_wire::CdpEvent;
use crate::service::browser::snapshot::Snapshot;
use crate::service::browser::{EventStream, PageChannel};
use crate::service::surface::runtime::{SurfaceNotice, SurfaceResume, SurfaceState};

const ATTACHED_PROFILE: &str = "attached";

pub(super) const PAGE_EVENTS: &[&str] = &[
    "Page.frameNavigated",
    "Page.navigatedWithinDocument",
    "Target.targetInfoChanged",
    "Page.javascriptDialogOpening",
    "Page.javascriptDialogClosed",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PageUpdate {
    Navigated { url: String },
    SameDocument { url: String },
    TargetInfo { title: String, url: String },
    Dialog(bool),
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
        "Page.javascriptDialogOpening" => Some(PageUpdate::Dialog(true)),
        "Page.javascriptDialogClosed" => Some(PageUpdate::Dialog(false)),
        _ => None,
    }
}

#[derive(Default)]
struct PageSession {
    bound_session: Option<String>,
    snapshot: Option<Snapshot>,
    dialog: bool,
    grab_ready: bool,
    listener: Option<tokio::task::AbortHandle>,
}

/// State a page listener and the runtime both write; one per browser surface instance.
pub(super) struct PageShared {
    state: watch::Sender<SurfaceState>,
    notices: broadcast::Sender<SurfaceNotice>,
    resume: watch::Sender<SurfaceResume>,
    session: Mutex<PageSession>,
}

impl PageShared {
    pub(super) fn new(owner: &str) -> Arc<Self> {
        let state = SurfaceState {
            owner: owner.to_string(),
            activity: IDLE.into(),
            needs_human: NEEDS_NONE.into(),
            profile: "clean".into(),
            ..Default::default()
        };
        Arc::new(Self {
            state: watch::channel(state).0,
            notices: broadcast::channel(16).0,
            resume: watch::channel(SurfaceResume::default()).0,
            session: Mutex::new(PageSession::default()),
        })
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

    pub(super) fn state(&self) -> SurfaceState {
        self.state.borrow().clone()
    }

    pub(super) fn subscribe_state(&self) -> watch::Receiver<SurfaceState> {
        self.state.subscribe()
    }

    pub(super) fn subscribe_notices(&self) -> broadcast::Receiver<SurfaceNotice> {
        self.notices.subscribe()
    }

    pub(super) fn update(&self, change: impl FnOnce(&mut SurfaceState)) {
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
            .and_then(crate::service::surface::sanitize_actor)
            .unwrap_or_default();
        self.update(|state| {
            state.activity = activity.to_string();
            state.actor = actor;
        });
    }

    /// Binds the listener to `page` once per CDP session; a relaunch brings a new session.
    pub(super) fn bind(self: &Arc<Self>, page: &PageChannel) {
        let events = {
            let mut session = self.session.lock();
            if session.bound_session.as_deref() == Some(page.session_id()) {
                return;
            }
            if let Some(listener) = session.listener.take() {
                listener.abort();
            }
            *session = PageSession {
                bound_session: Some(page.session_id().to_string()),
                ..Default::default()
            };
            self.update(|state| state.target_id = page.target_id().to_string());
            page.subscribe(PAGE_EVENTS)
        };
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
        let mut session = self.session.lock();
        if let Some(listener) = session.listener.take() {
            listener.abort();
        }
        *session = PageSession::default();
        self.update(|state| state.target_id.clear());
    }

    fn apply(&self, session_id: &str, update: PageUpdate) {
        let dialog = {
            let mut session = self.session.lock();
            if session.bound_session.as_deref() != Some(session_id) {
                return;
            }
            match &update {
                PageUpdate::Navigated { .. } => session.snapshot = None,
                PageUpdate::Dialog(open) => session.dialog = *open,
                PageUpdate::TargetInfo { .. } | PageUpdate::SameDocument { .. } => {}
            }
            session.dialog
        };
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
                PageUpdate::Dialog(_) => {}
            }
            state.needs_human = needs_human(&state.url, &state.title, dialog).to_string();
        });
    }

    pub(super) fn snapshot(&self) -> Option<Snapshot> {
        self.session.lock().snapshot.clone()
    }

    pub(super) fn set_snapshot(&self, snapshot: Snapshot) {
        self.session.lock().snapshot = Some(snapshot);
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

async fn listen(
    shared: Arc<PageShared>,
    mut events: EventStream,
    session_id: String,
    target_id: String,
) {
    while let Some(event) = events.recv().await {
        let event: &CdpEvent = &event;
        if let Some(update) = page_update(&event.method, &event.params, &target_id) {
            shared.apply(&session_id, update);
        }
    }
    let session = shared.session.lock();
    if session.bound_session.as_deref() == Some(session_id.as_str()) {
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
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn browser_page_connection_loss_notifies_each_bound_surface() {
        for owner in ["First", "Second"] {
            let shared = PageShared::new(owner);
            shared.session.lock().bound_session = Some("session".into());
            shared.update(|state| state.target_id = "target".into());
            let mut notices = shared.subscribe_notices();
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            drop(sender);
            listen(
                shared.clone(),
                EventStream::from_receiver(receiver),
                "session".into(),
                "target".into(),
            )
            .await;
            assert_eq!(
                notices.try_recv().unwrap().tone,
                Some(crate::service::surface::runtime::NoticeTone::Error)
            );
            assert_eq!(shared.state().target_id, "");
            assert_eq!(shared.state().owner, owner);
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            drop(sender);
            listen(
                shared.clone(),
                EventStream::from_receiver(receiver),
                "stale".into(),
                "target".into(),
            )
            .await;
            assert!(notices.try_recv().is_err());
        }
    }

    #[test]
    fn browser_page_main_frame_navigation_updates_url_and_subframes_do_not() {
        assert_eq!(
            page_update(
                "Page.frameNavigated",
                &json!({ "frame": { "id": "f", "url": "https://a.dev/x" } }),
                "t"
            ),
            Some(PageUpdate::Navigated {
                url: "https://a.dev/x".into()
            })
        );
        assert_eq!(
            page_update(
                "Page.frameNavigated",
                &json!({ "frame": { "id": "f", "url": "https://a.dev/x", "urlFragment": "#top" } }),
                "t"
            ),
            Some(PageUpdate::Navigated {
                url: "https://a.dev/x#top".into()
            })
        );
        assert_eq!(
            page_update(
                "Page.frameNavigated",
                &json!({ "frame": { "id": "c", "parentId": "f", "url": "https://ads" } }),
                "t"
            ),
            None
        );
    }

    #[test]
    fn browser_page_target_info_counts_only_our_page() {
        let info = |id: &str, kind: &str| json!({ "targetInfo": { "targetId": id, "type": kind, "title": "T", "url": "u" } });
        assert_eq!(
            page_update("Target.targetInfoChanged", &info("t", "page"), "t"),
            Some(PageUpdate::TargetInfo {
                title: "T".into(),
                url: "u".into()
            })
        );
        assert_eq!(
            page_update("Target.targetInfoChanged", &info("other", "page"), "t"),
            None
        );
        assert_eq!(
            page_update("Target.targetInfoChanged", &info("t", "iframe"), "t"),
            None
        );
        assert_eq!(
            page_update("Page.javascriptDialogOpening", &json!({}), "t"),
            Some(PageUpdate::Dialog(true))
        );
        assert_eq!(page_update("Page.loadEventFired", &json!({}), "t"), None);
    }

    #[test]
    fn browser_page_navigation_drops_refs_and_dialogs_flag_needs_human() {
        let shared = PageShared::new("Jev");
        shared.session.lock().bound_session = Some("s".into());
        shared.set_snapshot(Snapshot::default());
        shared.apply("s", PageUpdate::Dialog(true));
        assert_eq!(shared.state().needs_human, "dialog");
        assert!(shared.snapshot().is_some());
        shared.apply(
            "s",
            PageUpdate::Navigated {
                url: "https://a.dev".into(),
            },
        );
        assert!(shared.snapshot().is_none());
        shared.apply("s", PageUpdate::Dialog(false));
        assert_eq!(shared.state().needs_human, NEEDS_NONE);
        assert_eq!(shared.state().url, "https://a.dev");
        shared.apply(
            "stale-session",
            PageUpdate::Navigated {
                url: "https://old".into(),
            },
        );
        assert_eq!(shared.state().url, "https://a.dev");
    }

    #[test]
    fn browser_page_same_document_navigation_counts_only_the_main_frame() {
        assert_eq!(
            page_update(
                "Page.navigatedWithinDocument",
                &json!({ "frameId": "t", "url": "https://a.dev/#/inbox" }),
                "t"
            ),
            Some(PageUpdate::SameDocument {
                url: "https://a.dev/#/inbox".into()
            })
        );
        assert_eq!(
            page_update(
                "Page.navigatedWithinDocument",
                &json!({ "frameId": "child", "url": "https://ads/#x" }),
                "t"
            ),
            None
        );
    }

    #[test]
    fn browser_page_remembers_committed_resumable_pages_only() {
        let shared = PageShared::new("Jev");
        shared.seed_resume(SurfaceResume {
            url: Some("https://before.dev/".into()),
            profile: Some("work".into()),
        });
        let mut updates = shared.subscribe_resume();
        shared.session.lock().bound_session = Some("s".into());
        shared.apply(
            "s",
            PageUpdate::TargetInfo {
                title: "T".into(),
                url: "https://title-tick.dev/".into(),
            },
        );
        shared.apply(
            "s",
            PageUpdate::Navigated {
                url: "about:blank".into(),
            },
        );
        shared.apply(
            "stale",
            PageUpdate::Navigated {
                url: "https://stale.dev/".into(),
            },
        );
        assert!(!updates.has_changed().unwrap());
        assert_eq!(updates.borrow().url.as_deref(), Some("https://before.dev/"));
        shared.apply(
            "s",
            PageUpdate::Navigated {
                url: "https://a.dev/".into(),
            },
        );
        shared.apply(
            "s",
            PageUpdate::SameDocument {
                url: "https://a.dev/#/route".into(),
            },
        );
        assert!(updates.has_changed().unwrap());
        let resume = updates.borrow_and_update().clone();
        assert_eq!(resume.url.as_deref(), Some("https://a.dev/#/route"));
        assert_eq!(resume.profile.as_deref(), Some("work"));
        shared.remember_profile("clean");
        assert_eq!(updates.borrow_and_update().profile, None);
    }

    #[test]
    fn browser_page_attach_mode_remembers_nothing() {
        let shared = PageShared::new("Jev");
        shared.update(|state| state.profile = ATTACHED_PROFILE.into());
        shared.session.lock().bound_session = Some("s".into());
        shared.apply(
            "s",
            PageUpdate::Navigated {
                url: "https://app.local/".into(),
            },
        );
        shared.remember_profile(ATTACHED_PROFILE);
        assert_eq!(
            *shared.subscribe_resume().borrow(),
            SurfaceResume::default()
        );
    }

    #[test]
    fn browser_page_grab_setup_is_claimed_once_per_session() {
        let shared = PageShared::new("Jev");
        assert!(shared.claim_grab_setup());
        assert!(!shared.claim_grab_setup());
        assert!(shared.grab_ready());
        shared.reset_grab_setup();
        assert!(shared.claim_grab_setup());
    }
}
