use super::*;
use codeterm_plugin::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
};
use std::sync::mpsc;

struct HeldProvider {
    entered: mpsc::Sender<()>,
    release: parking_lot::Mutex<mpsc::Receiver<()>>,
}
impl AgentProvider for HeldProvider {
    fn id(&self) -> &str {
        "held-restart-provider"
    }
    fn display_name(&self) -> &str {
        "Held restart provider"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn auto_approve_flags(&self) -> Vec<String> {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        vec!["opaque-permission-flag".into()]
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn held_restart_provider_keeps_health_list_send_responsive_and_refuses_stale_snapshot() {
    for change_launch in [false, true] {
        let (mut mux, tab) = MultiplexerState::new();
        mux.link_pty(&tab, "restart-snapshot-pty".into());
        mux.get_tab_mut(&tab).unwrap().agent_type = Some("held-restart-provider".into());
        let fixture = ServiceFixture::new(mux);
        fixture.set_agent_full_access(true);
        let state = app_state(&fixture);
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (provider_entered, provider_ready) = mpsc::channel();
        let (release, receive) = mpsc::channel();
        state
            .svc
            .agent_registry
            .register_provider(Box::new(HeldProvider {
                entered: provider_entered,
                release: parking_lot::Mutex::new(receive),
            }));
        let service = state.svc.clone();
        let target = tab.clone();
        let snapshot =
            tokio::task::spawn_blocking(move || service.snapshot_restart_state_for_test(&target));
        tokio::task::spawn_blocking(move || {
            provider_ready.recv_timeout(Duration::from_secs(2)).unwrap();
            entered.send(()).unwrap();
        });
        ready.await.unwrap();
        let mux_available = state.svc.mux.try_write().is_some();
        if change_launch && mux_available {
            state
                .svc
                .mux
                .write()
                .get_tab_mut(&tab)
                .unwrap()
                .agent_session_id = Some("new-launch".into());
        }
        assert_health_list_send_respond(state.clone()).await;
        release.send(()).unwrap();
        let result = snapshot.await.unwrap();
        assert!(mux_available, "provider work must not retain mux");
        if change_launch {
            assert!(result.unwrap_err().contains("conflict"));
            assert!(!state.svc.mux.read().get_tab(&tab).unwrap().auto_approve);
        } else {
            result.unwrap();
            assert!(state.svc.mux.read().get_tab(&tab).unwrap().auto_approve);
        }
    }
}
