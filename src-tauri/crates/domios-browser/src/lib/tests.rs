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
