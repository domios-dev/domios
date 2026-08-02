use super::{json_str, orchestrator::SpawnAgentParams};
use crate::service::SpawnIdentityProvenance;

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
            provisioned_cwd: None,
            title: super::spawn_title::normalize_agent_title(json_str(params, "title")),
            title_provenance: json_str(params, "title_provenance")
                .as_deref()
                .map(parse_spawn_identity_provenance)
                .transpose()?
                .or_else(|| {
                    json_str(params, "title")
                        .filter(|value| !value.trim().is_empty())
                        .map(|_| SpawnIdentityProvenance::UserSupplied)
                }),
            description: json_str(params, "description")
                .map(|s| crate::service::normalize_tab_description(&s))
                .transpose()?
                .flatten(),
            role: json_str(params, "role"),
            role_profile: json_str(params, "role_profile"),
            parent_tab_id: json_str(params, "parent_tab_id").or_else(|| json_str(params, "parent")),
            plan_id: json_str(params, "plan_id").or_else(|| json_str(params, "plan")),
            can_spawn: params.get("can_spawn").and_then(|v| v.as_bool()),
            task: json_str(params, "task"),
            starter_prompt: Default::default(),
            task_id: json_str(params, "task_id"),
            task_launch_authority: None,
            plan_task: json_str(params, "plan_task").or_else(|| json_str(params, "plan-task")),
            execution_intent: params
                .get("execution_intent")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok()),
            provider_reserve_pct: params
                .get("provider_reserve_pct")
                .and_then(|value| value.as_f64())
                .map(|value| value as f32),
            workspace_id: json_str(params, "workspace_id"),
            force_tab: params.get("force_tab").and_then(|v| v.as_bool()),
            existing_tab_id: json_str(params, "existing_tab_id"),
            skip_orchestrator_onboarding: None,
            report_target_tab_id: json_str(params, "report_to")
                .or_else(|| json_str(params, "report_target_tab_id")),
            report_target_mesh_peer: json_str(params, "origin_peer")
                .or_else(|| json_str(params, "report_target_mesh_peer")),
            skip_permissions: params.get("skip_permissions").and_then(|v| v.as_bool()),
            spawned_by_tab_id: json_str(params, "spawned_by_tab_id"),
            model: json_str(params, "model"),
            planned_model: json_str(params, "planned_model"),
            reasoning_effort: json_str(params, "reasoning_effort")
                .or_else(|| json_str(params, "reasoning-effort")),
            system_prompt: json_str(params, "system_prompt").or_else(|| json_str(params, "system")),
            preset: json_str(params, "preset"),
            resume_session_id: json_str(params, "resume_session_id"),
            charter: json_str(params, "charter"),
            tick_interval_secs: json_u64(params, "tick_interval_secs"),
            nudge_cooldown_secs: json_u64(params, "nudge_cooldown_secs"),
            max_actions_per_tick: json_u64(params, "max_actions_per_tick")
                .and_then(|v| u32::try_from(v).ok()),
            activate: params.get("activate").and_then(|v| v.as_bool()),
            audience: params
                .get("audience")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            manager_bootstrap: None,
            structured_prompt_server: None,
        };
        out.normalize_role_profile()?;
        out.starter_prompt = crate::service::starter_prompt_for_role(out.role.as_deref());
        Ok(out)
    }
}

fn parse_spawn_identity_provenance(value: &str) -> Result<SpawnIdentityProvenance, String> {
    match value.trim() {
        "user_supplied" => Ok(SpawnIdentityProvenance::UserSupplied),
        "suggested" => Ok(SpawnIdentityProvenance::Suggested),
        other => Err(format!(
            "invalid title_provenance '{other}'; expected user_supplied|suggested"
        )),
    }
}

pub(super) fn normalize_spawn_params(params: &mut SpawnAgentParams) -> Result<(), String> {
    params.role = params
        .role
        .take()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    params.role_profile = params
        .role_profile
        .take()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
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
            params.role = Some("Manager".into());
            params.role_profile = Some("planner".into());
        }
        Some("Watcher") | Some("watcher") => {
            params.role = Some("Manager".into());
            params.role_profile = Some("watcher".into());
        }
        Some("Manager") => {
            params.role = Some("Manager".into());
            match profile {
                Some("planner" | "Planner") => params.role_profile = Some("planner".into()),
                Some("watcher" | "Watcher") => params.role_profile = Some("watcher".into()),
                other => {
                    return Err(format!(
                        "Manager requires a known role_profile (planner|watcher), got {other:?}"
                    ))
                }
            }
        }
        Some("Orchestrator" | "Conductor") => {
            params.role = Some("Orchestrator".into());
            if profile.is_some() {
                log::warn!("[spawn] ignoring role_profile on Orchestrator spawn");
            }
            params.role_profile = None;
        }
        Some("Worker") => {
            params.role = Some("Worker".into());
            if profile.is_some() {
                return Err(format!(
                    "Worker does not accept role_profile {:?}; use role Manager for planner|watcher",
                    profile
                ));
            }
            params.role_profile = None;
        }
        Some(other) => {
            return Err(format!(
                "Invalid role: {other}. Use: Orchestrator, Manager, Worker, Planner, Watcher"
            ));
        }
        None => {
            if profile.is_some() {
                return Err(format!(
                    "role_profile {:?} requires role Manager (or role Planner|Watcher sugar)",
                    profile
                ));
            }
            params.role_profile = None;
        }
    }
    Ok(())
}

fn json_u64(params: &serde_json::Value, key: &str) -> Option<u64> {
    params.get(key).and_then(|v| v.as_u64())
}
