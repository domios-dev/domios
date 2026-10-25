//! Claude Code launch + resume command construction.

use crate::provider::AgentLaunchParams;
use codeterm_util::shell::ShellSyntax;

const ASSIGNED_TASK_START_PROMPT: &str =
    "Execute the CodeTerm task provided in the appended system prompt. Follow its reporting instructions exactly.";
const IDLE_WAKEUP_START_PROMPT: &str =
    "You are a newly started agent. Wait for instructions from the user or your orchestration parent.";

pub(super) fn starter_prompt_text(prompt: crate::provider::StarterPrompt) -> Option<&'static str> {
    if let Some(text) = prompt.host_text() {
        return Some(text);
    }
    match prompt {
        crate::provider::StarterPrompt::NoStarter => None,
        crate::provider::StarterPrompt::AssignedTask => Some(ASSIGNED_TASK_START_PROMPT),
        crate::provider::StarterPrompt::IdleWakeup => Some(IDLE_WAKEUP_START_PROMPT),
        crate::provider::StarterPrompt::TeamBootstrap
        | crate::provider::StarterPrompt::GeneralBootstrap
        | crate::provider::StarterPrompt::StartupHandshake => unreachable!(),
    }
}

fn write_launch_prompt(task: &str) -> Option<String> {
    let rule = crate::agent_claude::launch_prompt_rule()?;
    let fragment = format!("{task}\n\n{rule}");
    let body = if cfg!(target_os = "windows") {
        format!("\u{feff}{fragment}")
    } else {
        fragment
    };
    let suffix = format!(".ct-task-{}", &uuid::Uuid::new_v4().to_string()[..8]);
    codeterm_util::task_files::write(&body, Some(&suffix))
}

pub(super) fn launch_spec(params: &AgentLaunchParams) -> Option<codeterm_types::agent::LaunchSpec> {
    launch_spec_with(params, super::context::launch_args, write_launch_prompt)
}

fn disallowed_tools(host_owned: crate::provider::HostOwnedCapabilities) -> Option<String> {
    let mut tools = Vec::new();
    if host_owned.questions {
        tools.push("AskUserQuestion");
    }
    if host_owned.todos {
        tools.push("TodoWrite");
    }
    (!tools.is_empty()).then(|| tools.join(","))
}

pub(super) fn provider_launch_spec(
    params: &crate::provider::LaunchSpecParams<'_>,
) -> Option<codeterm_types::agent::LaunchSpec> {
    match params {
        crate::provider::LaunchSpecParams::Fresh(params) => launch_spec(params),
        crate::provider::LaunchSpecParams::Resume(params) => {
            let mut args = vec!["--resume".into(), params.session_id.clone()];
            args.extend(params.args.clone());
            launch_spec(&AgentLaunchParams {
                args: Some(args),
                task: params.system_prompt.clone(),
                skip_permissions: params.skip_permissions,
                host_owned: params.host_owned,
                starter_prompt: crate::provider::StarterPrompt::NoStarter,
                ..Default::default()
            })
        }
    }
}

fn launch_spec_with(
    params: &AgentLaunchParams,
    launch_args: impl FnOnce(&[String]) -> Option<Vec<String>>,
    write_prompt: impl FnOnce(&str) -> Option<String>,
) -> Option<codeterm_types::agent::LaunchSpec> {
    let mut args = Vec::new();
    if params.skip_permissions {
        args.push("--dangerously-skip-permissions".into());
    }
    if let Some(name) = &params.agent_name {
        args.extend(["--agent".into(), name.clone()]);
    }
    if let Some(sid) = &params.session_id {
        args.extend(["--session-id".into(), sid.clone()]);
    }
    args.extend(launch_args(params.args.as_deref().unwrap_or_default())?);
    if let Some(tools) = disallowed_tools(params.host_owned) {
        args.extend(["--disallowedTools".into(), tools]);
    }
    let path = write_prompt(params.task.as_deref().unwrap_or_default())?;
    args.extend(["--append-system-prompt-file".into(), path]);
    if params.task.as_ref().is_some_and(|task| !task.is_empty()) {
        if let Some(prompt) = starter_prompt_text(params.starter_prompt) {
            let prompt = params
                .starter_prompt
                .system_prefix()
                .map(|prefix| {
                    format!(
                        "{prefix}{prompt}{}",
                        codeterm_wire::domios::DOMIOS_CLOSE_TAG
                    )
                })
                .unwrap_or_else(|| prompt.to_string());
            args.extend(["--".into(), prompt]);
        }
    }
    Some(codeterm_types::agent::LaunchSpec {
        program: "claude".into(),
        args,
        env_unset: vec!["CLAUDECODE".into()],
        ..Default::default()
    })
}

pub(super) fn build_launch_command(params: &AgentLaunchParams) -> String {
    let Some(spec) = launch_spec(params) else {
        return String::new();
    };
    let mut parts = vec![ShellSyntax::UNSET_CLAUDECODE.to_string(), spec.program];
    let mut previous = "";
    for arg in &spec.args {
        let quoted = match previous {
            "--" => ShellSyntax::quote_native_arg(arg),
            "--agent" | "--session-id" | "--append-system-prompt-file" => ShellSyntax::quote(arg),
            _ => ShellSyntax::quote_arg(arg),
        };
        parts.push(quoted);
        previous = arg;
    }
    parts.join(" ")
}

fn resume_base(session_id: &str, skip_permissions: bool) -> String {
    let mut parts = vec![
        ShellSyntax::UNSET_CLAUDECODE.to_string(),
        "claude".to_string(),
    ];
    if skip_permissions {
        parts.push("--dangerously-skip-permissions".to_string());
    }
    parts.push("--resume".to_string());
    parts.push(ShellSyntax::quote(session_id));
    parts.join(" ")
}

pub(super) fn build_resume_command(session_id: &str, skip_permissions: bool) -> Option<String> {
    Some(crate::provider::append_launch_args(
        resume_base(session_id, skip_permissions),
        &super::context::launch_args(&[])?,
    ))
}

/// `--resume` starts the stored session without the launch-time
/// `--append-system-prompt-file`, so the composed prompt is re-applied through
/// that same declared flag rather than a second channel.
pub(super) fn build_resume_command_with_context(
    params: &crate::provider::AgentResumeParams,
) -> Option<String> {
    let base = resume_base(&params.session_id, params.skip_permissions);
    let mut command =
        crate::provider::append_launch_args(base, &super::context::launch_args(&params.args)?);
    if let Some(tools) = disallowed_tools(params.host_owned) {
        command =
            crate::provider::append_launch_args(command, &["--disallowedTools".into(), tools]);
    }
    let path = write_launch_prompt(params.system_prompt.as_deref().unwrap_or_default())?;
    command = format!(
        "{command} --append-system-prompt-file {}",
        ShellSyntax::quote(&path)
    );
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{AgentLaunchParams, HostOwnedCapabilities};

    #[test]
    fn launch_spec_keeps_raw_arguments_and_materialization_failure_is_not_a_launch() {
        let params = AgentLaunchParams {
            agent_name: Some("two words; $value 😀".into()),
            session_id: Some("fixture-session".into()),
            args: Some(vec!["--resume".into(), "fixture-resume".into()]),
            skip_permissions: true,
            ..Default::default()
        };
        let spec = launch_spec_with(
            &params,
            |args| Some(args.to_vec()),
            |_| Some("prompt path".into()),
        )
        .unwrap();
        assert_eq!(
            spec.args,
            [
                "--dangerously-skip-permissions",
                "--agent",
                "two words; $value 😀",
                "--session-id",
                "fixture-session",
                "--resume",
                "fixture-resume",
                "--append-system-prompt-file",
                "prompt path"
            ]
        );
        assert_eq!(spec.env_unset.len(), 1);
        assert!(launch_spec_with(&params, |_| None, |_| panic!("failed args")).is_none());
        assert!(launch_spec_with(&params, |args| Some(args.to_vec()), |_| None).is_none());
    }

    #[test]
    fn host_owned_capabilities_disable_native_question_and_todo_tools() {
        let questions_only = AgentLaunchParams {
            host_owned: HostOwnedCapabilities {
                questions: true,
                todos: false,
            },
            ..Default::default()
        };
        let fresh = launch_spec_with(
            &questions_only,
            |_| Some(Vec::new()),
            |_| Some("prompt".into()),
        )
        .unwrap();
        assert!(fresh
            .args
            .windows(2)
            .any(|args| args == ["--disallowedTools", "AskUserQuestion"]));

        let both = HostOwnedCapabilities {
            questions: true,
            todos: true,
        };
        let resumed = crate::provider::append_launch_args(
            resume_base("own-session", false),
            &["--disallowedTools".into(), disallowed_tools(both).unwrap()],
        );
        assert!(resumed.contains("--disallowedTools AskUserQuestion,TodoWrite"));
        assert!(resumed.contains("--resume 'own-session'"));
    }

    #[test]
    fn test_build_launch_no_task() {
        let params = AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            host_owned: HostOwnedCapabilities::default(),
            session_id: None,
            structured_prompt_server: None,
            launch_marker: None,
            instance_id: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        if cfg!(target_os = "windows") {
            assert!(cmd.starts_with("Remove-Item Env:"));
            assert!(cmd.contains("[System.Text.Encoding]::UTF8"));
            assert!(cmd.contains(" claude --dangerously-skip-permissions"));
        } else {
            assert!(cmd.starts_with("unset CLAUDECODE && claude --dangerously-skip-permissions"));
        }
        assert!(cmd.contains(".ct-settings.json"));
        assert!(!cmd.contains("showThinkingSummaries"));
        assert_eq!(cmd.matches("--settings").count(), 1);
    }

    #[test]
    fn test_build_launch_with_task() {
        let params = AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: None,
            task: Some("line one\n<location>/repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод".to_string()),
            args: None,
            skip_permissions: true,
            host_owned: HostOwnedCapabilities::default(),
            session_id: None,
            structured_prompt_server: None,
            launch_marker: None,
            instance_id: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--dangerously-skip-permissions"));
        assert!(cmd.contains(".ct-task-"), "task via temp file");
        assert!(!cmd.contains(" -p"), "no headless -p mode anymore");
        assert!(
            cmd.contains(" -- "),
            "task positional must follow -- separator"
        );
        assert!(cmd.contains("--append-system-prompt-file"));
        assert!(!cmd.contains("Get-Content"));
        assert!(!cmd.contains("$(cat "));
        assert!(!cmd.contains("<location>") && !cmd.contains("whoami") && !cmd.contains("юникод"));
        assert!(cmd.contains(ASSIGNED_TASK_START_PROMPT));
        assert!(cmd.contains("<domios from=\"system\" context=\"assigned-task\">"));
        assert!(cmd.contains(codeterm_wire::domios::DOMIOS_CLOSE_TAG));
    }

    #[test]
    fn task_bound_starter_remains_byte_identical() {
        assert_eq!(
            starter_prompt_text(crate::provider::StarterPrompt::AssignedTask),
            Some(ASSIGNED_TASK_START_PROMPT)
        );
    }

    #[test]
    fn no_task_manager_starter_is_idle_only() {
        let params = AgentLaunchParams {
            starter_prompt: crate::provider::StarterPrompt::IdleWakeup,
            ..Default::default()
        };
        assert_eq!(
            starter_prompt_text(params.starter_prompt),
            Some(IDLE_WAKEUP_START_PROMPT)
        );
    }

    #[test]
    fn team_manager_starter_names_the_host_context() {
        assert_eq!(
            starter_prompt_text(crate::provider::StarterPrompt::TeamBootstrap),
            Some(crate::prompts::TEAM_BOOTSTRAP_STARTER.trim())
        );
    }

    #[test]
    fn team_manager_visible_turn_is_a_cli_safe_system_event() {
        let command = build_launch_command(&AgentLaunchParams {
            task: Some("system instructions".into()),
            starter_prompt: crate::provider::StarterPrompt::TeamBootstrap,
            ..Default::default()
        });
        assert!(command.contains("<domios from=\"system\" context=\"team-bootstrap\">"));
        assert!(command.contains(codeterm_wire::domios::DOMIOS_CLOSE_TAG));
        assert!(command.contains(crate::prompts::TEAM_BOOTSTRAP_STARTER.trim()));
    }

    #[test]
    fn system_instructions_can_launch_without_a_visible_starter_turn() {
        let params = AgentLaunchParams {
            task: Some("system instructions".into()),
            starter_prompt: crate::provider::StarterPrompt::NoStarter,
            ..Default::default()
        };
        let command = build_launch_command(&params);
        assert!(command.contains("--append-system-prompt-file"));
        assert!(!command.contains(" -- "));
    }

    #[test]
    fn test_build_launch_with_agent_name() {
        let params = AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: Some("architect".to_string()),
            task: None,
            args: None,
            skip_permissions: true,
            host_owned: HostOwnedCapabilities::default(),
            session_id: None,
            structured_prompt_server: None,
            launch_marker: None,
            instance_id: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--agent"));
        assert!(cmd.contains("'architect'"));
    }

    #[test]
    fn test_build_launch_visible_task_positional_after_flags_and_separator() {
        let model_id = codeterm_types::agent::models::resolve_model_id("haiku");
        let params = AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: None,
            task: Some("hello".to_string()),
            args: Some(vec!["--model".into(), model_id.into()]),
            skip_permissions: true,
            host_owned: HostOwnedCapabilities::default(),
            session_id: None,
            structured_prompt_server: None,
            launch_marker: None,
            instance_id: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains(".ct-task-"));
        assert!(cmd.contains(&format!("--model {}", model_id)));
        // Flags first, then file-backed task setup if needed, then `--`, then prompt last.
        let model_pos = cmd.find("--model").unwrap();
        let sep_pos = cmd.find(" -- ").unwrap();
        let task_pos = cmd.find(".ct-task-").unwrap();
        assert!(
            model_pos < task_pos && task_pos < sep_pos,
            "order must be flags -> task file flag -> --: {}",
            cmd
        );
    }

    #[test]
    fn test_build_launch_command_with_session_id_injection() {
        let params = AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            host_owned: HostOwnedCapabilities::default(),
            session_id: Some("my-pre-generated-uuid".to_string()),
            structured_prompt_server: None,
            launch_marker: None,
            instance_id: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        assert!(
            cmd.contains("--session-id"),
            "must pass --session-id flag, got: {}",
            cmd
        );
        assert!(
            cmd.contains("my-pre-generated-uuid"),
            "must include the chosen uuid, got: {}",
            cmd
        );
    }

    #[test]
    fn test_session_id_injected_even_with_task() {
        // Interactive launch always honors --session-id (the old -p path that ignored it is gone).
        let params = AgentLaunchParams {
            context_files: None,
            tool_less_instructions_path: None,
            agent_name: None,
            task: Some("hello".to_string()),
            args: None,
            skip_permissions: true,
            host_owned: HostOwnedCapabilities::default(),
            session_id: Some("sid-xyz".to_string()),
            structured_prompt_server: None,
            launch_marker: None,
            instance_id: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        assert!(cmd.contains("--session-id"));
        assert!(cmd.contains("sid-xyz"));
        let sid_pos = cmd.find("--session-id").unwrap();
        let sep_pos = cmd.find(" -- ").unwrap();
        assert!(
            sid_pos < sep_pos,
            "session-id before task separator: {}",
            cmd
        );
    }

    #[test]
    fn test_build_resume_command() {
        let cmd = build_resume_command("abc-def-123-456", true).unwrap();
        assert!(cmd.contains("--resume"));
        assert!(cmd.contains("abc-def-123-456"));
        assert!(cmd.contains("--dangerously-skip-permissions"));
    }
}
