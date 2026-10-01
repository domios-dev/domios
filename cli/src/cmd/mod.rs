pub mod agent;
mod agent_spawn;
pub mod bridge;
pub mod capabilities;
pub mod cron;
pub mod delayed;
pub mod docs;
pub mod dream;
pub mod effort;
pub mod error_contract;
pub mod essence_map;
pub mod graph_hubs;
pub mod graph_mutate;
pub mod hints;
pub mod identify;
pub mod install;
pub mod integration;
pub mod local_io;
pub mod manifest;
pub mod mem;
pub mod mem_secret;
pub mod mem_secret_autounlock;
pub mod mesh;
pub mod model_error;
pub mod notify;
pub mod orchestrator;
pub mod pad;
pub mod paste;
pub mod payload_source;
pub mod persona;
pub mod plan;
pub mod plan_doctor;
pub mod plan_doctor_competition;
pub mod plan_elements;
pub mod plan_errors;
pub mod plan_files;
pub mod plan_handoff;
pub mod plan_heartbeat;
pub mod plan_in_progress;
pub mod plan_lanes;
pub mod plan_linking;
pub mod plan_local;
pub mod plan_markdown;
pub mod plan_next;
pub mod plan_output;
pub mod plan_prune;
pub mod plan_reconcile;
pub mod plan_record;
pub mod plan_render;
pub mod plan_sections;
pub mod plan_skill;
pub mod plan_state;
pub mod plan_tags;
pub mod plan_task_retag;
pub mod plan_task_update;
pub mod plugin;
pub mod print;
pub mod quoted_handoff;
pub mod report_attribution;
pub mod report_body;
pub mod report_guard;
pub mod report_lane;
pub mod resolved_text;
pub mod send_text;
pub mod service;
pub mod sessions;
pub mod spec;
pub mod system;
pub mod tab;
mod tab_restart;
pub mod task;
pub mod task_list_render;
pub mod team;
pub mod test_plan;
pub mod theme;
pub mod update;
pub mod usage;
pub mod web;
pub mod worker;
mod workspace_map;

pub(crate) fn render_workspace_map(
    workspaces: &serde_json::Value,
    tabs: Option<&serde_json::Value>,
    teams: Option<&serde_json::Value>,
    plans: Option<&serde_json::Value>,
    caller_workspace_id: Option<&str>,
    caller_tab_id: Option<&str>,
) -> String {
    workspace_map::render(
        workspaces,
        tabs,
        teams,
        plans,
        caller_workspace_id,
        caller_tab_id,
    )
}
