use super::{json_str, orchestrator::SpawnAgentParams};

impl SpawnAgentParams {
    pub fn from_json(params: &serde_json::Value) -> Result<Self, String> {
        let mut out = Self {
            agent: json_str(params, "agent"),
            agent_name: json_str(params, "agent_name"),
            args: params.get("args").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            }),
            cwd: json_str(params, "cwd"),
            title: super::spawn_title::normalize_agent_title(json_str(params, "title")),
            description: json_str(params, "description")
                .map(|s| crate::service::normalize_pane_description(&s))
                .transpose()?
                .flatten(),
            role: json_str(params, "role"),
            role_profile: json_str(params, "role_profile"),
            parent_pane_id: json_str(params, "parent_pane_id")
                .or_else(|| json_str(params, "parent")),
            can_spawn: params.get("can_spawn").and_then(|v| v.as_bool()),
            task: json_str(params, "task"),
            workspace_id: json_str(params, "workspace_id"),
            force_tab: params.get("force_tab").and_then(|v| v.as_bool()),
            report_target_pane_id: json_str(params, "report_to")
                .or_else(|| json_str(params, "report_target_pane_id")),
            report_target_mesh_peer: json_str(params, "origin_peer")
                .or_else(|| json_str(params, "report_target_mesh_peer")),
            skip_permissions: params.get("skip_permissions").and_then(|v| v.as_bool()),
            spawned_by_pane_id: json_str(params, "spawned_by_pane_id"),
            model: json_str(params, "model"),
            system_prompt: json_str(params, "system_prompt").or_else(|| json_str(params, "system")),
            preset: json_str(params, "preset"),
            resume_session_id: json_str(params, "resume_session_id"),
            activate: params.get("activate").and_then(|v| v.as_bool()),
            structured_prompt_server: None,
        };
        out.normalize_role_profile();
        Ok(out)
    }
}

pub(super) fn normalize_spawn_params(params: &mut SpawnAgentParams) {
    let role = params
        .role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let profile = params
        .role_profile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match role {
        Some("Planner") | Some("planner") => {
            params.role = Some("Worker".into());
            params.role_profile = Some("planner".into());
        }
        Some("Worker") | None if matches!(profile, Some("Planner" | "planner")) => {
            params.role = Some("Worker".into());
            params.role_profile = Some("planner".into());
        }
        Some("Orchestrator" | "Conductor") => {
            if profile.is_some() {
                log::warn!("[spawn] ignoring role_profile on Orchestrator spawn");
            }
            params.role_profile = None;
        }
        Some("Worker") => {
            if profile.is_some() {
                log::warn!(
                    "[spawn] unknown role_profile {:?}; spawning generic Worker",
                    profile
                );
            }
            params.role_profile = None;
        }
        Some(other) => {
            log::warn!(
                "[spawn] unknown role profile '{}'; spawning generic Worker",
                other
            );
            params.role = Some("Worker".into());
            params.role_profile = None;
        }
        None => {
            if profile.is_some() {
                log::warn!(
                    "[spawn] unknown role_profile {:?}; spawning generic Worker",
                    profile
                );
                params.role = Some("Worker".into());
            }
            params.role_profile = None;
        }
    }
}
