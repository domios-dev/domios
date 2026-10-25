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

#[test]
fn native_diagnostics_port_is_never_published_for_production() {
    let port = super::NativeDiagnosticsPort(std::sync::Arc::new(|| Some(9398)));
    assert_eq!(port.for_build(false), "");
    assert_eq!(port.for_build(true), "9398");
    let invalid = super::NativeDiagnosticsPort(std::sync::Arc::new(|| Some(0)));
    assert_eq!(invalid.for_build(true), "");
}

#[tokio::test]
async fn health_reads_live_native_diagnostics_and_clears_detached_port() {
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    use std::sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    };
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    assert_eq!(
        super::health(axum::extract::State(state.clone())).await.0["instance"]["cdp_port"],
        ""
    );
    let port = Arc::new(AtomicU16::new(9398));
    let attached = port.clone();
    assert!(state
        .svc
        .register_host_extension(Arc::new(super::NativeDiagnosticsPort(Arc::new(
            move || {
                match attached.load(Ordering::Acquire) {
                    0 => None,
                    port => Some(port),
                }
            }
        ))))
        .is_ok());
    assert_eq!(
        super::health(axum::extract::State(state.clone())).await.0["instance"]["cdp_port"],
        if cfg!(debug_assertions) { "9398" } else { "" }
    );
    port.store(0, Ordering::Release);
    assert_eq!(
        super::health(axum::extract::State(state)).await.0["instance"]["cdp_port"],
        ""
    );
}
