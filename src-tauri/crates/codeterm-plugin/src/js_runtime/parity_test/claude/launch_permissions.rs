use super::*;

#[test]
fn launch_permission_capability_matches_native_and_js() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let declared = js
        .launch_permission_capability()
        .expect("the plugin declares launch permissions");
    assert_eq!(
        native.launch_permission_capability(),
        Some(declared.clone())
    );
    for model in native.available_models() {
        assert!(
            declared.model_runs_unattended(&model.id).is_some(),
            "every catalogued model has a declared mode: {}",
            model.id
        );
    }
    assert!(!declared.allow_command_args(&["opaque command"]).is_empty());
    assert!(!declared.deny_native_subagent_args().is_empty());
}

#[test]
fn resume_quotes_only_args_a_shell_would_reinterpret() {
    if require_plugins().is_none() {
        return;
    }
    let (n, j) = pair();
    let spaced = "Rule(opaque token:*)";
    let params = crate::provider::AgentResumeParams {
        session_id: "abc-def-123-456".into(),
        skip_permissions: false,
        agent_name: None,
        system_prompt: None,
        instance_id: None,
        args: vec!["--flag".into(), spaced.into()],
    };
    let quoted = codeterm_util::shell::ShellSyntax::quote(spaced);
    for (label, command) in [
        (
            "native",
            n.build_resume_command_with_context(&params).unwrap(),
        ),
        ("js", j.build_resume_command_with_context(&params).unwrap()),
    ] {
        assert!(
            command.contains(&format!(" --flag {quoted}")),
            "{label}: {command}"
        );
    }
}
