use super::*;

#[tokio::test]
async fn duplicate_wake_updates_keep_one_live_route_attempt() {
    let initial = RouteAttempt {
        sequence: 1,
        lifecycle: RouteLifecycle::Waking { deadline_ms: 100 },
    };
    let awake = RouteAttempt {
        lifecycle: RouteLifecycle::Awake { since_ms: Some(20) },
        ..initial
    };
    let (attempts, changes) = watch::channel(initial);
    let mut observer = attempts.subscribe();
    let (entered, mut dials) = tokio::sync::mpsc::unbounded_channel();
    let (finish, release) = watch::channel(false);

    let route = RouteTask {
        route: CloudRoute::ProviderProxy {
            host: "wake.fly.dev".into(),
        },
        epoch: 1,
        attempts,
        task: tokio::spawn(run_route_attempts(changes, async move |attempt| {
            entered.send(attempt).unwrap();
            let mut release = release.clone();
            release.wait_for(|finish| *finish).await.unwrap();
        })),
    };

    tokio::time::timeout(Duration::from_secs(5), async {
        assert_eq!(dials.recv().await, Some(initial));
        route.update_attempt(initial);
        assert!(!observer.has_changed().unwrap());

        route.update_attempt(awake);
        assert_eq!(*observer.borrow_and_update(), awake);
        route.update_attempt(awake);
        assert!(!observer.has_changed().unwrap());
        assert!(dials.try_recv().is_err());

        finish.send(true).unwrap();
        assert_eq!(dials.recv().await, Some(awake));
        route.stop().await;
        assert_eq!(dials.recv().await, None);
    })
    .await
    .expect("route attempts should advance only after the active attempt ends");
}
