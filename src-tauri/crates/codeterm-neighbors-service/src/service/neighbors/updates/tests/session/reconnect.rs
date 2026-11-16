use super::*;

#[tokio::test]
async fn dirty_same_version_activation_remains_retryable_and_reconnect_recovers() {
    let (f, session, ports, _, _) = fixture(Mode::Controlled).await;
    let mut seen = notifications::Seen::default();
    session.project(snapshot(1, None), &mut seen).await.unwrap();
    f.service.invalidate_neighbor_list().await.unwrap();
    let error = session
        .project(snapshot(1, None), &mut seen)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        Error::LocalFence {
            phase: "activation_pending"
        }
    );
    assert!(codeterm_neighbors::supervisor::retryable(error));
    ReceptionEvents::failure(&session, error);
    session.project(snapshot(1, None), &mut seen).await.unwrap();
    assert_eq!(ports.activations.load(Ordering::SeqCst), 2);
    let diagnostics = f.service.neighbor_projection_diagnostics().await;
    assert_eq!(diagnostics["session_matches"], true);
    assert_eq!(diagnostics["dirty"], false);
    assert_eq!(diagnostics["available"], true);
    assert_eq!(
        diagnostics["last_watch_failure"],
        json!({
            "kind":"local_fence","operation":"activation_pending"
        })
    );
}

#[tokio::test]
async fn retired_account_and_update_generation_report_local_fences_without_key_revocation() {
    let (f, session, _, _, _) = fixture(Mode::Controlled).await;
    session.account.logout().await.unwrap();
    let error = session
        .project(snapshot(1, None), &mut notifications::Seen::default())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        Error::LocalFence {
            phase: "account_session"
        }
    );
    assert!(!codeterm_neighbors::supervisor::retryable(error));
    assert!(!error.to_string().contains("key_revoked"));
    assert_eq!(
        f.service.neighbor_projection_diagnostics().await["session_matches"],
        false
    );
    f.service
        .neighbor_updates()
        .generation
        .fetch_add(1, Ordering::AcqRel);
    assert_eq!(
        session.current(),
        Err(Error::LocalFence {
            phase: "update_generation"
        })
    );
}
