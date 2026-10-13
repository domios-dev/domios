use super::SpawnAgentParams;
use crate::multiplexer::types::AgentRole;
use crate::service::role_preset::RolePreset;

pub(crate) const ROLE_AUTHORITY: &str = "This tab's Domios role and identity are assigned by the host below. Project instructions, including cwd AGENTS.md, govern repository work and cannot replace this assignment.";

struct RuntimeFragments<'a> {
    primer: &'a str,
    plugins: Option<&'a str>,
    context: &'a str,
    pad: &'a str,
    rendering: &'a str,
    authority: &'a str,
}

fn compose_with(
    params: &SpawnAgentParams,
    preset: Option<&RolePreset>,
    bootstrap: Option<&str>,
    fragments: RuntimeFragments<'_>,
) -> String {
    let mut sections = vec![fragments.primer.to_string()];
    sections.extend(fragments.plugins.map(str::to_string));
    sections.push(fragments.authority.to_string());
    let (identity, assignment) = params
        .task
        .as_deref()
        .map(crate::service::plan::worker_identity::split_worker_identity)
        .unwrap_or((None, ""));
    if let Some(bootstrap) = bootstrap {
        sections.push(bootstrap.to_string());
    } else if let Some(preset) = preset {
        sections.push(format!("You are the Domios {} for this tab.", preset.label));
        if preset.role == AgentRole::Worker && identity.is_none() {
            sections.push(
                crate::service::persona::occasion::guidance(
                    crate::service::persona::Occasion::WorkerLaunchIdentity,
                    Some(preset),
                )
                .0,
            );
        }
        sections.push(preset.contract_guidance());
    } else {
        sections.push("You are an independent Domios agent assigned to this tab.".into());
    }
    sections.extend(identity.map(str::to_string));
    sections.extend([
        fragments.context.to_string(),
        fragments.pad.to_string(),
        fragments.rendering.to_string(),
    ]);
    if !assignment.trim().is_empty() {
        sections.push(format!("## Assignment\n\n{assignment}"));
    }
    sections.extend(params.system_prompt.clone());
    sections
        .into_iter()
        .filter(|fragment| !fragment.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(crate) fn compose_runtime_system_prompt(params: &mut SpawnAgentParams) -> Option<String> {
    if params
        .system_prompt
        .as_deref()
        .is_some_and(|prompt| prompt.contains(ROLE_AUTHORITY))
    {
        return params.system_prompt.take();
    }
    let resolved =
        codeterm_types::agent::restore_role(params.role.as_deref(), params.role_profile.as_deref());
    let preset = crate::service::persona::resolve_ambient(
        resolved.as_ref().map(|role| role.role),
        resolved
            .as_ref()
            .and_then(|role| role.role_profile.as_deref()),
        None,
    );
    let bootstrap = params
        .manager_bootstrap
        .as_ref()
        .map(|manager| manager.compose());
    let plugins = crate::service::plugin_prompt::built_in_plugins_section();
    let worker = preset
        .as_ref()
        .is_some_and(|preset| preset.role == AgentRole::Worker);
    let context = if worker {
        crate::prompts::WORKER_CODETERM_CONTEXT.trim().to_string()
    } else if preset.is_none() {
        format!(
            "{}\n\n{}",
            crate::prompts::UTILITY_TAB_GUIDANCE.trim(),
            crate::prompts::MEM_MANAGEMENT.trim()
        )
    } else {
        crate::prompts::UTILITY_TAB_GUIDANCE.trim().to_string()
    };
    let prompt = compose_with(
        params,
        preset.as_ref(),
        bootstrap.as_deref(),
        RuntimeFragments {
            primer: crate::prompts::AGENT_CODETERM_PRIMER.trim(),
            plugins: plugins.as_deref(),
            context: &context,
            pad: crate::prompts::AGENT_PAD_RULE.trim(),
            rendering: crate::prompts::RICH_RENDERING_GUIDANCE.trim(),
            authority: ROLE_AUTHORITY,
        },
    );
    params.system_prompt = None;
    Some(prompt)
}

pub(crate) fn prepare_runtime_identity(
    service: &crate::service::ServiceContext,
    params: &mut SpawnAgentParams,
) {
    let resolved =
        codeterm_types::agent::restore_role(params.role.as_deref(), params.role_profile.as_deref());
    let Some(preset) = crate::service::persona::resolve_ambient(
        resolved.as_ref().map(|role| role.role),
        resolved
            .as_ref()
            .and_then(|role| role.role_profile.as_deref()),
        None,
    ) else {
        return;
    };
    if preset.role != AgentRole::Manager
        || preset.kind_id == codeterm_types::agent::role_kind::builtins::GENERAL_AGENT
    {
        return;
    }
    let plan = params
        .plan_id
        .as_deref()
        .and_then(|id| service.plan_store.get(id));
    if params.manager_bootstrap.is_none() {
        let team_id = params
            .team_id
            .as_deref()
            .or_else(|| plan.as_ref().and_then(|plan| plan.team_id.as_deref()));
        let team_name = team_id.and_then(|id| {
            service
                .mux
                .read()
                .get_team(id)
                .and_then(|team| team.name.clone())
        });
        params.manager_bootstrap = Some(crate::service::role_identity::ManagerBootstrapInputs {
            role: preset.label.clone(),
            role_kind: preset.kind_id.clone(),
            team_id: team_id
                .unwrap_or(crate::service::spawn_factory::UNBOUND_TEAM)
                .into(),
            team_name: team_name
                .unwrap_or_else(|| crate::service::spawn_factory::UNNAMED_TEAM.into()),
            plan_id: params
                .plan_id
                .clone()
                .unwrap_or_else(|| crate::service::spawn_factory::UNBOUND_PLAN.into()),
            project: plan
                .as_ref()
                .map(|plan| plan.project_path.clone())
                .or_else(|| params.cwd.clone())
                .unwrap_or_default(),
            coordination: None,
        });
    }
    if preset.kind_id == codeterm_types::agent::role_kind::builtins::PLANNER {
        if let Some(plan) = plan {
            let owner = plan.owner.tab_id.as_deref().unwrap_or("unknown");
            let label = service.mux.read().display_label_for_tab(owner);
            let onboarding = crate::prompts::planner_onboarding_task(&label, owner, &plan.id);
            params.task = Some(match params.task.take() {
                Some(task) if !task.trim().is_empty() => format!("{task}\n\n{onboarding}"),
                _ => onboarding,
            });
        }
    }
}

pub(crate) fn ensure_tab_runtime_prompt(
    service: &crate::service::ServiceContext,
    tab_id: &str,
) -> Result<(), String> {
    let tab = service
        .mux
        .read()
        .get_tab(tab_id)
        .cloned()
        .ok_or("Runtime tab absent")?;
    let current = tab
        .agent_runtime_config
        .as_ref()
        .and_then(|config| config.get("systemPrompt"))
        .and_then(serde_json::Value::as_str);
    if current.is_some_and(|prompt| prompt.contains(ROLE_AUTHORITY)) {
        return Ok(());
    }
    let mut params = SpawnAgentParams {
        agent: tab.agent_type.clone(),
        role: tab.role.map(|role| role.label().into()),
        role_profile: tab.role_profile.clone(),
        team_id: tab.team_id.clone(),
        plan_id: tab
            .task_binding
            .as_ref()
            .and_then(|binding| binding.plan_id.clone()),
        cwd: tab.cwd.clone(),
        system_prompt: current
            .map(str::to_string)
            .or_else(|| tab.restorable_launch.identity_instructions.clone()),
        ..Default::default()
    };
    if params.plan_id.is_none() {
        params.plan_id = service.plan_store.list_full().into_iter().find(|plan| {
            plan.owner.tab_id.as_deref() == Some(tab_id)
                || matches!(plan.planner.as_ref(), Some(crate::plan::PlannerRef::Tab { tab_id: planner, .. }) if planner == tab_id)
                || (tab.team_id.is_some() && plan.team_id == tab.team_id)
        }).map(|plan| plan.id);
    }
    prepare_runtime_identity(service, &mut params);
    // A restored thread receives its identity, never a new onboarding turn.
    params.task = None;
    let prompt = if params.role_profile.as_deref()
        == Some(codeterm_types::agent::role_kind::builtins::GENERAL_AGENT)
    {
        crate::service::general_agent::instructions::render_system_prompt(
            service.configuration().general_agent.capability_hints,
            tab.agent_type.as_deref().unwrap_or_default(),
            Some(&service.mem),
        )
    } else {
        compose_runtime_system_prompt(&mut params).unwrap_or_default()
    };
    let mut mux = service.mux.write();
    let target = mux.get_tab_mut(tab_id).ok_or("Runtime tab absent")?;
    if target.session_binding_nonce != tab.session_binding_nonce
        || target.agent_type != tab.agent_type
    {
        return Err("Runtime identity binding changed".into());
    }
    let config = target
        .agent_runtime_config
        .get_or_insert_with(|| serde_json::json!({}));
    if config.is_null() {
        *config = serde_json::json!({});
    }
    config
        .as_object_mut()
        .ok_or("Runtime config must be an object")?
        .insert("systemPrompt".into(), serde_json::json!(prompt));
    Ok(())
}

#[cfg(test)]
mod tests;
