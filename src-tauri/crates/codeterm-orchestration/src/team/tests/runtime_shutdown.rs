use super::*;
use codeterm_service_core::service::tab::RuntimeStopOverride;
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::Arc;

struct FakeRuntimeEngine {
    live: Mutex<BTreeSet<String>>,
    stopped: Mutex<Vec<String>>,
    close_thread: std::thread::ThreadId,
    fail: bool,
}

fn stop_runtime(service: &ServiceContext, tab_id: &str) -> Result<(), String> {
    let engine = service.host_extension::<FakeRuntimeEngine>().unwrap();
    assert_ne!(std::thread::current().id(), engine.close_thread);
    assert!(service.mux.try_write().is_some());
    assert!(engine.live.lock().remove(tab_id));
    engine.stopped.lock().push(tab_id.to_string());
    if engine.fail {
        Err("injected stop failure".into())
    } else {
        Ok(())
    }
}

async fn close_team_with_fake_runtimes(fail: bool) {
    let (mut mux, team_id, orchestrator, planner, worker) = team_members();
    let workspace = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let survivor = mux.create_tab(&workspace).unwrap();
    let members = BTreeSet::from([orchestrator, planner, worker]);
    let engine = Arc::new(FakeRuntimeEngine {
        live: Mutex::new(members.iter().cloned().chain([survivor.clone()]).collect()),
        stopped: Mutex::new(Vec::new()),
        close_thread: std::thread::current().id(),
        fail,
    });
    let root = tempfile::tempdir().unwrap();
    let instance = crate::test_fixture::open_instance_with_mux(root.path(), mux);
    crate::test_fixture::install_team_close_ports(&instance, root.path());
    let service = &instance.fixture.service;
    assert!(service.register_host_extension(engine.clone()).is_ok());
    assert!(service
        .register_host_extension(Arc::new(RuntimeStopOverride(stop_runtime)))
        .is_ok());

    let result = service
        .close_team(&team_id, TeamMutationCaller::Human, true)
        .await;

    if fail {
        let error = result.unwrap_err();
        assert!(error.contains("Team runtime shutdown failed"), "{error}");
        for member in &members {
            assert!(error.contains(member), "{error}");
        }
    } else {
        result.unwrap();
    }
    let stopped = engine.stopped.lock();
    assert_eq!(stopped.len(), members.len());
    assert_eq!(stopped.iter().cloned().collect::<BTreeSet<_>>(), members);
    assert_eq!(*engine.live.lock(), BTreeSet::from([survivor.clone()]));
    {
        let mux = service.mux.read();
        assert!(mux.get_team(&team_id).is_none());
        assert!(members.iter().all(|member| mux.get_tab(member).is_none()));
        assert!(mux.get_tab(&survivor).is_some());
    }
    drop(stopped);
    codeterm_service_core::persistence_test_support::close(instance);
}

#[tokio::test]
async fn team_close_stops_every_member_runtime_off_the_async_thread() {
    close_team_with_fake_runtimes(false).await;
}

#[tokio::test]
async fn team_close_attempts_every_runtime_when_stops_fail() {
    close_team_with_fake_runtimes(true).await;
}

#[tokio::test]
async fn single_tab_close_uses_the_same_runtime_shutdown() {
    let (mux, tab_id) = MultiplexerState::new();
    let fixture = crate::test_fixture::ServiceFixture::new(mux);
    let engine = Arc::new(FakeRuntimeEngine {
        live: Mutex::new(BTreeSet::from([tab_id.clone()])),
        stopped: Mutex::new(Vec::new()),
        close_thread: std::thread::current().id(),
        fail: false,
    });
    let service = &fixture.service;
    assert!(service.register_host_extension(engine.clone()).is_ok());
    assert!(service
        .register_host_extension(Arc::new(RuntimeStopOverride(stop_runtime)))
        .is_ok());

    service.close_tab_async(&tab_id).await.unwrap();

    assert!(engine.live.lock().is_empty());
    assert_eq!(*engine.stopped.lock(), vec![tab_id.clone()]);
    assert!(service.mux.read().get_tab(&tab_id).is_none());
}
