use super::*;
use codeterm_plugin::provider::LaunchPermissionCapability;

const PROVIDER: &str = "unattended-fixture";
const MANUAL_MODEL: &str = "manual-model";
const AUTO_MODEL: &str = "auto-model";

fn capability(grants_commands: bool) -> LaunchPermissionCapability {
    LaunchPermissionCapability {
        unattended_mode_models: vec![AUTO_MODEL.into()],
        command_rule_format: grants_commands.then(|| "Rule[{command}]".into()),
        allow_rule_args_format: vec!["--allow-rule".into(), "{rule}".into()],
        deny_tool_args_format: vec!["--deny-tool".into(), "{tool}".into()],
        native_subagent_tools: vec!["SubagentTool".into()],
    }
}

fn registry(capability: Option<LaunchPermissionCapability>) -> AgentRegistry {
    let registry = AgentRegistry::new();
    let mut provider = crate::agent::test_provider::MockProvider::new(PROVIDER);
    if let Some(capability) = capability {
        provider = provider.with_launch_permissions(capability);
    }
    registry.register_provider(Box::new(provider));
    registry
}

fn launch<'a>(model: &'a str, role: Option<&'a str>) -> UnattendedLaunch<'a> {
    UnattendedLaunch {
        provider_id: PROVIDER,
        model: Some(model),
        role,
        has_task: true,
        ..Default::default()
    }
}

fn rule(command: &str) -> String {
    format!("Rule[{command}]")
}

#[test]
fn role_launch_requires_unattended_codeterm_capability() {
    let registry = registry(Some(capability(true)));

    let orchestrator = launch(MANUAL_MODEL, Some("Orchestrator"));
    let refusal = preflight(&registry, &orchestrator).expect_err("managing role is refused");
    assert!(refusal.contains(MANUAL_MODEL), "{refusal}");
    assert!(refusal.contains(AUTO_MODEL), "{refusal}");
    let planner = UnattendedLaunch {
        role: Some("Manager"),
        role_profile: Some("planner"),
        ..launch(MANUAL_MODEL, None)
    };
    assert!(preflight(&registry, &planner).is_err());

    let worker = launch(MANUAL_MODEL, Some("Worker"));
    preflight(&registry, &worker).expect("a worker receives its commands instead");
    let granted = grant_args(&registry, &worker);
    for command in LIFECYCLE_COMMANDS {
        assert!(granted.contains(&rule(command)), "{command}: {granted:?}");
    }
    assert!(!granted.iter().any(|arg| arg == "SubagentTool"));

    let unattended = launch(AUTO_MODEL, Some("Orchestrator"));
    preflight(&registry, &unattended).expect("the model runs unattended");
    assert_eq!(
        grant_args(&registry, &unattended),
        vec!["--deny-tool", "SubagentTool"],
        "an Orchestrator never gets native subagents"
    );

    let full_access = UnattendedLaunch {
        full_access: true,
        ..orchestrator
    };
    preflight(&registry, &full_access).expect("full access runs everything unattended");
    assert_eq!(
        grant_args(&registry, &full_access),
        vec!["--deny-tool", "SubagentTool"]
    );
    assert!(grant_args(&registry, &planner)
        .iter()
        .all(|arg| arg != "SubagentTool"));
}

#[test]
fn role_launch_without_a_command_grant_mechanism_is_refused() {
    let registry = registry(Some(capability(false)));
    let worker = launch(MANUAL_MODEL, Some("Worker"));
    let refusal = preflight(&registry, &worker).expect_err("nothing can grant the commands");
    assert!(refusal.contains(MANUAL_MODEL), "{refusal}");

    let undeclared = self::registry(None);
    preflight(&undeclared, &launch(MANUAL_MODEL, Some("Orchestrator")))
        .expect("an undeclared provider makes no claim");
    assert!(grant_args(&undeclared, &worker).is_empty());
}

#[test]
fn interactive_launch_owes_no_commands() {
    let registry = registry(Some(capability(false)));
    let interactive = UnattendedLaunch {
        has_task: false,
        ..launch(MANUAL_MODEL, None)
    };
    preflight(&registry, &interactive).expect("a plain tab owes CodeTerm nothing");
    assert!(grant_args(&registry, &interactive).is_empty());
}

#[test]
fn cron_agent_allowlist_or_refusal() {
    let cron = UnattendedLaunch {
        purpose: LaunchPurpose::CronRun,
        ..launch(MANUAL_MODEL, None)
    };
    let grants = registry(Some(capability(true)));
    preflight(&grants, &cron).expect("the run receives its completion command");
    let granted = grant_args(&grants, &cron);
    for command in CRON_RUN_COMMANDS.iter().chain(LIFECYCLE_COMMANDS) {
        assert!(granted.contains(&rule(command)), "{command}: {granted:?}");
    }

    let unattended = UnattendedLaunch {
        model: Some(AUTO_MODEL),
        ..cron
    };
    assert!(grant_args(&grants, &unattended).is_empty());

    let refusal = preflight(&registry(Some(capability(false))), &cron)
        .expect_err("a run that cannot complete unattended is refused");
    assert!(refusal.contains(MANUAL_MODEL), "{refusal}");
}

#[test]
fn launch_command_carries_the_granted_commands() {
    let registry = registry(Some(capability(true)));
    let params = crate::service::SpawnAgentParams {
        agent: Some(PROVIDER.into()),
        model: Some(MANUAL_MODEL.into()),
        role: Some("Worker".into()),
        ..Default::default()
    };
    let command = crate::service::orchestrator::build_agent_command(
        PROVIDER,
        &params,
        Some("opaque task"),
        None,
        &registry,
    );
    for command_prefix in LIFECYCLE_COMMANDS {
        assert!(command.contains(&rule(command_prefix)), "{command}");
    }
}
