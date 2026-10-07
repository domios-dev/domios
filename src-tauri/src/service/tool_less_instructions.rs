use crate::agent::provider::{AgentLaunchParams, AgentProvider, SystemPromptDelivery};

pub(crate) fn build(provider: &dyn AgentProvider, params: &AgentLaunchParams) -> String {
    let Some(params) = prepare(
        provider.is_tool_less(),
        provider.system_prompt_delivery(),
        params,
        crate::prompts::TOOL_LESS,
        || codeterm_util::task_files::write(crate::prompts::TOOL_LESS, Some(".tool-less.md")),
    ) else {
        log::error!("[launch] mandatory tool-less instructions could not be materialized");
        return String::new();
    };
    provider.build_launch_command(&params)
}

fn prepare(
    tool_less: bool,
    delivery: SystemPromptDelivery,
    params: &AgentLaunchParams,
    fragment: &str,
    materialize: impl FnOnce() -> Option<String>,
) -> Option<AgentLaunchParams> {
    let mut params = params.clone();
    if tool_less {
        match delivery {
            SystemPromptDelivery::External => {
                if params.tool_less_instructions_path.is_none() {
                    params.tool_less_instructions_path = Some(materialize()?);
                }
            }
            _ => params.task = Some(compose(fragment, params.task.as_deref())),
        }
    }
    Some(params)
}

fn compose(fragment: &str, task: Option<&str>) -> String {
    task.map_or_else(
        || fragment.to_string(),
        |task| format!("{fragment}\n\n{task}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn class_and_declared_delivery_select_the_shared_fragment_path() {
        let params = AgentLaunchParams {
            task: Some("assignment".into()),
            ..Default::default()
        };
        let ordinary = prepare(
            false,
            SystemPromptDelivery::External,
            &params,
            "fragment",
            || panic!("ordinary provider must not materialize class rules"),
        )
        .unwrap();
        assert_eq!(ordinary.task, params.task);
        assert!(ordinary.tool_less_instructions_path.is_none());
        let external = prepare(
            true,
            SystemPromptDelivery::External,
            &params,
            "fragment",
            || Some("opaque-path".into()),
        )
        .unwrap();
        assert_eq!(external.task, params.task);
        assert_eq!(
            external.tool_less_instructions_path.as_deref(),
            Some("opaque-path")
        );
        assert!(prepare(
            true,
            SystemPromptDelivery::External,
            &params,
            "fragment",
            || None
        )
        .is_none());
        for delivery in [
            SystemPromptDelivery::CliFlag("opaque-flag".into()),
            SystemPromptDelivery::SessionConfig,
        ] {
            let supplied = prepare(true, delivery, &params, "fragment", || {
                panic!("native delivery does not materialize a file")
            })
            .unwrap();
            assert_eq!(supplied.task.as_deref(), Some("fragment\n\nassignment"));
        }
    }
    #[test]
    fn shared_fragment_preserves_assignment_and_stays_within_budget() {
        assert!(crate::prompts::TOOL_LESS.len() <= 1024);
        assert_eq!(
            compose("opaque fragment", Some("opaque task")),
            "opaque fragment\n\nopaque task"
        );
        assert_eq!(compose("opaque fragment", None), "opaque fragment");
    }
}
