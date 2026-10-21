use crate::agent::{chat_types::ChatMsg, reports::AgentReport};
use crate::multiplexer::state::MultiplexerSnapshot;
pub use codeterm_worker_services::watcher::observe::{
    collect_watch_group, is_manager_watcher, observation_config_from_json, resolve_orchestrator_id,
    ObservationConfig, WatchGroup,
};
use std::collections::HashMap;
pub struct ObservationSources<'a> {
    pub chat_tail: &'a dyn Fn(&str, usize) -> Vec<ChatMsg>,
    pub reports: &'a [AgentReport],
    pub last_activity_ms: &'a HashMap<String, u64>,
}
pub fn build_observation(
    snapshot: &MultiplexerSnapshot,
    tab_id: &str,
    config: &ObservationConfig,
    sources: &ObservationSources<'_>,
) -> serde_json::Value {
    use codeterm_worker_services::watcher::observe as leaf;
    let reports: Vec<_> = sources
        .reports
        .iter()
        .filter(|_| config.reports)
        .map(|r| leaf::ReportObservation {
            id: &r.id,
            from_tab_id: &r.from_tab_id,
            from_title: &r.from_title,
            message: &r.message,
            timestamp: r.timestamp,
            status: r.status,
        })
        .collect();
    leaf::build_observation(
        snapshot,
        tab_id,
        config,
        &leaf::ObservationSources {
            chat_tail: sources.chat_tail,
            reports: &reports,
            last_activity_ms: sources.last_activity_ms,
        },
    )
}
