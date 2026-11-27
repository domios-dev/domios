use super::*;

fn route_task(attempt: RouteAttempt) -> (RouteTask, watch::Receiver<RouteDial>) {
    let (attempts, changes) = watch::channel(RouteDial {
        attempt,
        redial: false,
    });
    let route = RouteTask {
        route: CloudRoute::ProviderProxy {
            host: "wake.fly.dev".into(),
        },
        epoch: 1,
        attempts,
        end: Default::default(),
        task: tokio::spawn(std::future::pending()),
    };
    (route, changes)
}

#[tokio::test]
async fn duplicate_wake_updates_keep_one_live_route_attempt() {
    let wake = RouteAttempt {
        sequence: 1,
        lifecycle: RouteLifecycle::Waking {
            deadline_ms: 100,
            ingress_ready_at: 1,
        },
    };
    let awake = RouteAttempt {
        lifecycle: RouteLifecycle::Awake { since_ms: Some(20) },
        ..wake
    };
    let (route, mut dials) = route_task(wake);

    route.rearm(wake, || None);
    route.rearm(awake, || Some("live-link".into()));
    assert!(!dials.has_changed().unwrap());

    *route.end.lock() = Some(RouteEnd {
        dial: *dials.borrow(),
        attached: true,
        link_nonce: Some("live-link".into()),
    });
    route.rearm(awake, || Some("live-link".into()));
    assert!(dials.has_changed().unwrap());
    assert_eq!(dials.borrow_and_update().attempt, awake);
    route.rearm(awake, || Some("live-link".into()));
    assert!(!dials.has_changed().unwrap());
}

#[tokio::test]
async fn awake_route_end_rearms_once_then_waits_for_fresh_link_evidence() {
    let attempt = RouteAttempt {
        sequence: 1,
        lifecycle: RouteLifecycle::Awake { since_ms: Some(20) },
    };
    let (route, mut dials) = route_task(attempt);
    *route.end.lock() = Some(RouteEnd {
        dial: *dials.borrow(),
        attached: true,
        link_nonce: Some("closed-link".into()),
    });
    route.rearm(attempt, || Some("closed-link".into()));
    assert!(dials.has_changed().unwrap());
    let redial = *dials.borrow_and_update();
    assert!(redial.redial);
    assert_eq!(redial.attempt, attempt);
    route.rearm(attempt, || Some("closed-link".into()));
    assert!(!dials.has_changed().unwrap());

    *route.end.lock() = Some(RouteEnd {
        dial: redial,
        attached: false,
        link_nonce: Some("closed-link".into()),
    });
    route.rearm(attempt, || Some("closed-link".into()));
    assert!(!dials.has_changed().unwrap());
    route.rearm(attempt, || Some("external-link".into()));
    assert!(dials.has_changed().unwrap());
    assert!(!dials.borrow_and_update().redial);
}

#[tokio::test]
async fn legacy_wake_refusal_waits_for_new_readiness_evidence() {
    let legacy = RouteAttempt {
        sequence: 1,
        lifecycle: RouteLifecycle::LegacyWake { deadline_ms: 100 },
    };
    let (route, mut dials) = route_task(legacy);
    *route.end.lock() = Some(RouteEnd {
        dial: *dials.borrow(),
        attached: false,
        link_nonce: None,
    });
    route.rearm(legacy, || None);
    assert!(!dials.has_changed().unwrap());

    let ready = RouteAttempt {
        lifecycle: RouteLifecycle::Waking {
            deadline_ms: 100,
            ingress_ready_at: 1,
        },
        ..legacy
    };
    route.rearm(ready, || None);
    assert!(dials.has_changed().unwrap());
    assert_eq!(dials.borrow_and_update().attempt, ready);
}
