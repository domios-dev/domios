use codeterm_plugin::{
    host_traits::PluginHostCtx,
    js_runtime::{call_json, PluginVm},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use std::sync::{mpsc, Arc};
use std::time::Duration;

struct HeldMemory {
    entered: mpsc::Sender<()>,
    release: parking_lot::Mutex<mpsc::Receiver<()>>,
}

impl PluginHostCtx for HeldMemory {
    fn mem_search_json(&self, _: &str) -> String {
        self.entered.send(()).unwrap();
        self.release.lock().recv().unwrap();
        serde_json::json!({"hits":[]}).to_string()
    }

    fn analytics_query_json(&self, _: &str) -> String {
        "null".into()
    }
    fn workspace_ensure_json(&self, _: &str) -> String {
        "null".into()
    }
    fn workspace_tabs_json(&self, _: &str) -> String {
        "[]".into()
    }
    fn close_tab(&self, _: &str) {}
    fn spawn_tab_json(&self, _: &str) -> String {
        "null".into()
    }
    fn worker_get_json(&self, _: &str) -> Option<String> {
        None
    }
    fn worker_kill(&self, _: &str) {}
    fn worker_pick_color(&self) -> String {
        String::new()
    }
    fn worker_register_json(&self, _: &str) {}
    fn worker_start_json(&self, _: &str, _: &str) -> String {
        "null".into()
    }
    fn worker_poll_json(&self, _: &str) -> String {
        "null".into()
    }
    fn agent_spawn_json(&self, _: &str, _: &str, _: &str) -> String {
        "null".into()
    }
    fn agent_send_json(&self, _: &str, _: &str, _: &str) -> String {
        "null".into()
    }
    fn agent_reap_json(&self, _: &str) -> String {
        "null".into()
    }
    fn agent_poll_json(&self, _: &str) -> String {
        "null".into()
    }
    fn progress_raw_json(&self, _: &str, _: &str) -> String {
        "null".into()
    }
    fn contributions_changed(&self, _: &str) {}
    fn emit_graph_worker_spawned_json(&self, _: &str) {}
    fn socket_display_path(&self) -> String {
        String::new()
    }
    fn api_base_url(&self) -> String {
        String::new()
    }
    fn secret_get(&self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn secret_set(&self, _: &str, _: &str, _: &str) -> bool {
        false
    }
    fn secret_delete(&self, _: &str, _: &str) -> bool {
        false
    }
}

struct Release(Option<mpsc::Sender<()>>);
impl Release {
    fn now(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.now();
    }
}

#[test]
fn production_health_answers_on_one_worker_while_provider_host_io_is_held() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut release = Release(Some(release_tx));
    let host: Arc<dyn PluginHostCtx> = Arc::new(HeldMemory {
        entered: entered_tx,
        release: parking_lot::Mutex::new(release_rx),
    });
    let vm = Arc::new(PluginVm::from_source_with(
        "api-held-provider-io",
        "module.exports={read(){return host.io({op:'memSearch',options:{}},function(value){return value;});},pure(){return 'healthy';}};".into(),
        Some(serde_json::json!({"id":"api-held-provider-io","version":"0.0.0","displayName":"Fixture","hostIoJobs":true}).to_string()),
    ).unwrap());
    assert!(vm.set_service(Arc::downgrade(&host)));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let read_vm = vm.clone();
        let read =
            tokio::spawn(async move { call_json::<serde_json::Value>(&read_vm, "read", &[]) });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let health =
            tokio::spawn(async move { super::health(axum::extract::State(state)).await.0 });
        let health_result = tokio::time::timeout(Duration::from_secs(1), health).await;
        let pure_vm = vm.clone();
        let pure = tokio::spawn(async move { call_json::<String>(&pure_vm, "pure", &[]) });
        let pure_result = tokio::time::timeout(Duration::from_secs(1), pure).await;
        release.now();
        assert_eq!(health_result.unwrap().unwrap()["startup"]["state"], "ready");
        assert_eq!(pure_result.unwrap().unwrap().unwrap(), "healthy");
        assert_eq!(read.await.unwrap().unwrap()["hits"], serde_json::json!([]));
    });
}
