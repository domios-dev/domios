use super::server_stop_owes_relaunch;
use crate::application_host::ServerStopError;

#[tokio::test]
async fn reservation_refusal_never_dispatches_server_shutdown() {
    struct Stop(std::sync::atomic::AtomicUsize);
    #[async_trait::async_trait]
    impl crate::application_host::ServerStop for Stop {
        async fn stop_server(&self) -> Result<(), ServerStopError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }
    let stop = Stop(std::sync::atomic::AtomicUsize::new(0));
    let result = super::stop_reserved_server(
        &stop,
        std::future::ready(Err("Build in Project became busy".into())),
    )
    .await;
    let error = result.unwrap_err();
    assert!(!error.dispatched);
    assert!(error.reason.contains("Build in Project"));
    assert_eq!(stop.0.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[tokio::test]
async fn only_a_stop_refused_before_dispatch_releases_the_reservation() {
    struct Stop {
        dispatched: bool,
        released: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl crate::application_host::ServerStop for Stop {
        async fn stop_server(&self) -> Result<(), ServerStopError> {
            Err(ServerStopError {
                dispatched: self.dispatched,
                reason: "stop refused".into(),
            })
        }
        async fn release_install_shutdown(&self, id: &str) -> Result<(), String> {
            assert_eq!(id, "reserved-operation");
            self.released
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }
    for dispatched in [false, true] {
        let stop = Stop {
            dispatched,
            released: std::sync::atomic::AtomicUsize::new(0),
        };
        let result = super::stop_reserved_server(
            &stop,
            std::future::ready(Ok(Some("reserved-operation".into()))),
        )
        .await;
        assert_eq!(result.unwrap_err().dispatched, dispatched);
        assert_eq!(
            stop.released.load(std::sync::atomic::Ordering::SeqCst),
            usize::from(!dispatched)
        );
    }
}

#[test]
fn confirmed_server_stop_owes_a_relaunch_after_a_failed_install() {
    assert!(server_stop_owes_relaunch(&Ok(())));
}

#[test]
fn refusal_before_dispatch_keeps_the_running_pair_without_relaunch() {
    let refused = Err(ServerStopError {
        dispatched: false,
        reason: "opaque-refusal".into(),
    });
    assert!(!server_stop_owes_relaunch(&refused));
}

#[test]
fn unconfirmed_exit_after_dispatch_still_owes_a_relaunch() {
    let uncertain = Err(ServerStopError {
        dispatched: true,
        reason: "opaque-uncertainty".into(),
    });
    assert!(server_stop_owes_relaunch(&uncertain));
}
