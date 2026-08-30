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
        | crate::provider::StarterPrompt::StartupHandshake => unreachable!(),
    }
}

pub(super) fn build_launch_command(params: &AgentLaunchParams) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(ShellSyntax::UNSET_CLAUDECODE.to_string());
    parts.push("claude".to_string());
    if params.skip_permissions {
        parts.push("--dangerously-skip-permissions".to_string());
    }

    if let Some(ref name) = params.agent_name {
        parts.push("--agent".to_string());
        parts.push(ShellSyntax::quote(name));
    }

    // Pre-generated session id (only honored for fresh sessions — `--session-id` is silently ignored by claude when also resuming).
    if let Some(ref sid) = params.session_id {
        parts.push("--session-id".to_string());
        parts.push(ShellSyntax::quote(sid));
    }

    if let Some(ref extra) = params.args {
        parts.extend(extra.iter().cloned());
    }

    // Keep large worker prompts out of shell parsing and argv on every platform.
    if let Some(ref task) = params.task {
        let path = ShellSyntax::task_file_path(task);
        parts.push("--append-system-prompt-file".to_string());
        parts.push(ShellSyntax::quote(&path));
        if let Some(prompt) = starter_prompt_text(params.starter_prompt) {
            let prompt = params
                .starter_prompt
                .system_prefix()
                .map(|prefix| format!(" {prefix}{prompt}"))
                .unwrap_or_else(|| prompt.to_string());
            parts.push("--".to_string());
            parts.push(ShellSyntax::quote(&prompt));
        }
    }

    parts.join(" ")
}

pub(super) fn build_resume_command(session_id: &str, skip_permissions: bool) -> Option<String> {
    let mut parts = vec![
        ShellSyntax::UNSET_CLAUDECODE.to_string(),
        "claude".to_string(),
    ];
    if skip_permissions {
        parts.push("--dangerously-skip-permissions".to_string());
    }
    parts.push("--resume".to_string());
    parts.push(ShellSyntax::quote(session_id));
    Some(parts.join(" "))
}

/// `--resume` starts the stored session without the launch-time
/// `--append-system-prompt-file`, so the composed prompt is re-applied through
/// that same declared flag rather than a second channel.
pub(super) fn build_resume_command_with_context(
    params: &crate::provider::AgentResumeParams,
) -> Option<String> {
    let base = build_resume_command(&params.session_id, params.skip_permissions)?;
    let mut command = crate::provider::append_launch_args(base, &params.args);
    if let Some(prompt) = params
        .system_prompt
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        let path = ShellSyntax::task_file_path(prompt);
        command = format!(
            "{command} --append-system-prompt-file {}",
            ShellSyntax::quote(&path)
        );
    }
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::AgentLaunchParams;

    #[test]
    fn test_build_launch_no_task() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            session_id: None,
            structured_prompt_server: None,
            starter_prompt: Default::default(),
        };
        let cmd = build_launch_command(&params);
        if cfg!(target_os = "windows") {
            assert!(cmd.starts_with("Remove-Item Env:CLAUDECODE"));
            assert!(cmd.contains("[System.Text.Encoding]::UTF8"));
            assert!(cmd.ends_with(" claude --dangerously-skip-permissions"));
        } else {
            assert_eq!(
                cmd,
                "unset CLAUDECODE && claude --dangerously-skip-permissions"
            );
        }
    }

    #[test]
    fn test_build_launch_with_task() {
        let params = AgentLaunchParams {
            agent_name: None,
            task: Some("line one\n<location>/repo</location>\n'quotes' \"double\" $HOME `whoami` --flag\nюникод".to_string()),
            args: None,
            skip_permissions: true,
            session_id: None,
            structured_prompt_server: None,
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
        assert!(cmd.contains("-=-codeterm:system context=assigned-task-=-"));
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
        assert!(command.contains("-=-codeterm:system context=team-bootstrap-=-"));
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
            agent_name: Some("architect".to_string()),
            task: None,
            args: None,
            skip_permissions: true,
            session_id: None,
            structured_prompt_server: None,
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
            agent_name: None,
            task: Some("hello".to_string()),
            args: Some(vec!["--model".into(), model_id.into()]),
            skip_permissions: true,
            session_id: None,
            structured_prompt_server: None,
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
            agent_name: None,
            task: None,
            args: None,
            skip_permissions: true,
            session_id: Some("my-pre-generated-uuid".to_string()),
            structured_prompt_server: None,
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
            agent_name: None,
            task: Some("hello".to_string()),
            args: None,
            skip_permissions: true,
            session_id: Some("sid-xyz".to_string()),
            structured_prompt_server: None,
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
