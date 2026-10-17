use super::*;

pub(super) fn assert_installed_descriptors() {
    let Some(root) = require_plugins() else {
        return;
    };
    let task = "adapter-task-fixture";
    let mut plugin_dirs = std::fs::read_dir(root)
        .expect("read bundled plugin directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("provider-"))
        .filter(|entry| declares_agent_provider(&entry.path().join("plugin.json")))
        .collect::<Vec<_>>();
    plugin_dirs.sort_by_key(|entry| entry.file_name());

    assert!(
        !plugin_dirs.is_empty(),
        "installed descriptor matrix is empty"
    );
    for entry in plugin_dirs {
        let manifest = entry.path().join("plugin.json");
        let provider = load_one(&manifest)
            .unwrap_or_else(|error| panic!("load {}: {error}", manifest.display()));
        let descriptor = provider.describe();
        let command = provider.build_launch_command(&AgentLaunchParams {
            task: Some(task.into()),
            instance_id: Some("instance-parity".into()),
            ..Default::default()
        });

        assert!(
            !command.trim().is_empty(),
            "{} launch command",
            descriptor.id
        );
        let task_delivered_by_pty = matches!(
            provider.task_delivery(),
            crate::provider::TaskDelivery::PtyInput
        );
        match provider.task_delivery() {
            crate::provider::TaskDelivery::InlineFlag => {}
            crate::provider::TaskDelivery::PtyInput => {
                assert!(
                    !command.contains(task),
                    "{} PTY delivery must preserve first-message fallback",
                    descriptor.id
                );
            }
        }
        match provider.system_prompt_delivery() {
            crate::provider::SystemPromptDelivery::External if task_delivered_by_pty => {
                assert!(
                    !command.contains(task),
                    "{} external delivery must reserve confirmed fallback",
                    descriptor.id
                );
            }
            crate::provider::SystemPromptDelivery::External => {}
            crate::provider::SystemPromptDelivery::CliFlag(mechanism) => {
                let marker = mechanism
                    .split_whitespace()
                    .last()
                    .expect("CLI system prompt mechanism must not be empty");
                assert!(
                    command.contains(marker),
                    "{} system delivery must use declared mechanism {marker}",
                    descriptor.id
                );
                let params = AgentLaunchParams {
                    task: Some(task.into()),
                    instance_id: Some("instance-parity".into()),
                    ..Default::default()
                };
                if let Some(spec) = provider
                    .launch_spec(&crate::provider::LaunchSpecParams::Fresh(&params))
                    .filter(|spec| {
                        spec.args
                            .iter()
                            .any(|arg| arg.starts_with(&format!("{marker}=")))
                    })
                {
                    let flag = mechanism.split_whitespace().next().unwrap();
                    assert!(
                        spec.args
                            .windows(2)
                            .any(|pair| pair[0] == flag && pair[1] == format!("{marker}={task}")),
                        "{} delivers the task through one declared config argument",
                        descriptor.id
                    );
                } else {
                    assert!(
                        !command.contains(task),
                        "{} system delivery must not use fallback text",
                        descriptor.id
                    );
                }
            }
            crate::provider::SystemPromptDelivery::SessionConfig => {
                assert!(
                    !command.contains(task),
                    "{} session config must not use fallback text",
                    descriptor.id
                );
            }
        }
    }
}

/// Chat-backend-only plugins (remote tasks) have no PTY launch to compare.
fn declares_agent_provider(manifest: &std::path::Path) -> bool {
    std::fs::read_to_string(manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| value["capabilities"]["agentProvider"].is_object())
}
