use super::IDENTIFY_FEATURES;

#[test]
fn identify_advertises_remote_task_dispatch_capability() {
    assert!(IDENTIFY_FEATURES.contains(&super::REMOTE_TASK_DISPATCH_FEATURE));
}

#[test]
fn identify_advertises_delayed_recurrence_capability() {
    assert!(IDENTIFY_FEATURES.contains(&"delayed_recurrence"));
}

mod display_name {
    use axum::extract::State;
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use futures_util::FutureExt;

    use crate::test_support::app_state;

    async fn health_instance(state: &crate::routes::AppState) -> serde_json::Value {
        let body = super::super::health(State(state.clone())).await.0;
        body["instance"].clone()
    }

    #[tokio::test]
    async fn health_reports_the_mesh_identity_name_without_mesh_peers() {
        let fixture = ServiceFixture::new(MultiplexerState::empty());
        let state = app_state(&fixture);
        state.mesh.lock().ident.instance_name = "BLACKSTAR [dev]".into();
        assert_eq!(
            health_instance(&state).await["display_name"],
            "BLACKSTAR [dev]"
        );
    }

    #[tokio::test]
    async fn health_reports_no_name_instead_of_waiting_on_a_held_mesh_lock() {
        let fixture = ServiceFixture::new(MultiplexerState::empty());
        let state = app_state(&fixture);
        let _held = state.mesh.lock();
        let instance = health_instance(&state)
            .now_or_never()
            .expect("health must complete without yielding while the mesh lock is held");
        assert_eq!(instance["display_name"], "");
    }
}
