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
            Some(crate::surface::runtime::NoticeTone::Error)
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
    let capture = shared.begin_snapshot("s").unwrap();
    shared
        .finish_snapshot(&capture, Snapshot::default())
        .unwrap();
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

#[test]
fn browser_snapshot_rejects_old_capture_and_session() {
    let shared = PageShared::new("Agent");
    shared.session.lock().bound_session = Some("s".into());
    assert!(shared.begin_snapshot("old-session").is_err());
    let old = shared.begin_snapshot("s").unwrap();
    let new = shared.begin_snapshot("s").unwrap();
    use base64::Engine;
    assert_eq!(new.len(), 22);
    assert_eq!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&new)
            .unwrap()
            .len(),
        16
    );
    assert_ne!(old, new);
    assert!(shared.finish_snapshot(&old, Snapshot::default()).is_err());
    assert!(shared.snapshot().is_none());
    shared.finish_snapshot(&new, Snapshot::default()).unwrap();
    assert!(shared.snapshot().is_some());
}

#[test]
fn browser_same_document_navigation_revokes_refs_and_pending_capture() {
    let shared = PageShared::new("Agent");
    shared.session.lock().bound_session = Some("s".into());
    let first = shared.begin_snapshot("s").unwrap();
    shared.finish_snapshot(&first, Snapshot::default()).unwrap();
    shared.apply(
        "s",
        PageUpdate::SameDocument {
            url: "https://app.dev/#new".into(),
        },
    );
    assert!(shared.snapshot().is_none());
    let pending = shared.begin_snapshot("s").unwrap();
    shared.apply(
        "s",
        PageUpdate::Navigated {
            url: "https://app.dev/other".into(),
        },
    );
    assert!(shared
        .finish_snapshot(&pending, Snapshot::default())
        .is_err());
}

#[test]
fn browser_viewer_input_revokes_refs_and_an_inflight_capture() {
    let shared = PageShared::new("Agent");
    shared.session.lock().bound_session = Some("s".into());
    let first = shared.begin_snapshot("s").unwrap();
    shared.finish_snapshot(&first, Snapshot::default()).unwrap();
    shared.invalidate_snapshot();
    assert!(shared.snapshot().is_none());
    let pending = shared.begin_snapshot("s").unwrap();
    shared.invalidate_snapshot();
    assert!(shared
        .finish_snapshot(&pending, Snapshot::default())
        .is_err());
}
