use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::host_traits::PluginHostCtx;
use crate::js_runtime::host_api::PluginSandbox;
use crate::js_runtime::manifest::PermissionsConfig;
use crate::js_runtime::vm::{call_json, PluginVm};

#[test]
fn agent_resume_echoes_session_id() {
    let v: serde_json::Value = serde_json::from_str(&agent_resume_json("sess-7")).unwrap();
    assert_eq!(v["sessionId"], "sess-7");
}

#[test]
fn agent_get_is_null_host_keeps_no_key_map() {
    assert_eq!(agent_get_json(), "null");
}

#[test]
fn agent_usage_is_null_until_wired() {
    assert_eq!(agent_usage_json(), "null");
}

/// Answers analytics with the query it received and a missing total; every other
/// host surface is inert.
struct AnalyticsHost;

impl PluginHostCtx for AnalyticsHost {
    fn mem_search_json(&self, _: &str) -> String {
        String::new()
    }
    fn analytics_query_json(&self, query_json: &str) -> String {
        let query: Value = serde_json::from_str(query_json).unwrap_or(Value::Null);
        json!({ "echo": query, "tokens": { "totals": null, "series": [] } }).to_string()
    }
    fn workspace_ensure_json(&self, _: &str) -> String {
        String::new()
    }
    fn workspace_tabs_json(&self, _: &str) -> String {
        String::new()
    }
    fn close_tab(&self, _: &str) {}
    fn spawn_tab_json(&self, _: &str) -> String {
        String::new()
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
        String::new()
    }
    fn worker_poll_json(&self, _: &str) -> String {
        String::new()
    }
    fn agent_spawn_json(&self, _: &str, _: &str, _: &str) -> String {
        String::new()
    }
    fn agent_send_json(&self, _: &str, _: &str, _: &str) -> String {
        String::new()
    }
    fn agent_reap_json(&self, _: &str) -> String {
        String::new()
    }
    fn agent_poll_json(&self, _: &str) -> String {
        String::new()
    }
    fn progress_raw_json(&self, _: &str, _: &str) -> String {
        String::new()
    }
    fn contributions_changed(&self, _: &str) {}
    fn emit_graph_worker_spawned_json(&self, _: &str) {}
    fn socket_display_path(&self) -> String {
        String::new()
    }
    fn api_base_url(&self) -> String {
        String::new()
    }
    fn api_token(&self) -> Option<String> {
        None
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

const PROBE_PLUGIN: &str =
    r#"module.exports.default = { probe(query) { return host.analytics.query(query); } }"#;

fn probe_vm(perms: &PermissionsConfig, host: &Arc<dyn PluginHostCtx>) -> PluginVm {
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms("analytics-probe", perms),
    );
    let vm =
        PluginVm::from_source_sandboxed("analytics-probe", PROBE_PLUGIN.to_string(), None, sandbox)
            .expect("probe vm");
    assert!(vm.set_service(Arc::downgrade(host)));
    vm
}

#[test]
fn analytics_host_query_needs_the_analytics_grant_and_keeps_nulls() {
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost);
    let query = json!({ "group_by": "workspace" });

    let denied = probe_vm(&PermissionsConfig::default(), &host);
    let refused: Value = call_json(&denied, "probe", &[query.clone()]).unwrap();
    assert!(refused["error"].is_string());
    assert!(
        refused.get("echo").is_none(),
        "a confined plugin without the grant never reaches the host"
    );

    let granted = probe_vm(
        &PermissionsConfig {
            analytics: true,
            ..Default::default()
        },
        &host,
    );
    let reply: Value = call_json(&granted, "probe", &[query]).unwrap();
    assert_eq!(reply["echo"]["group_by"], "workspace");
    assert!(
        reply["tokens"]["totals"].is_null(),
        "a missing figure crosses the bridge as null, never zero"
    );

    let system = PluginSandbox::unrestricted()
        .with_identity_and_perms("system-plugin", &PermissionsConfig::default());
    assert!(system.analytics_allowed());
}
