use super::{SpawnTabSpec, TabCommand};

pub(super) fn prepare_tab_identity(spec: &mut SpawnTabSpec) -> Result<(), String> {
    if spec.shell.is_some() && !matches!(spec.command, TabCommand::Shell) {
        return Err("shell profiles require PTY shell backing; agents run natively".into());
    }
    if spec.purpose != crate::multiplexer::types::ShellPurpose::Shell
        && !matches!(spec.command, TabCommand::Shell)
    {
        return Err("utility shell purpose requires PTY shell backing".into());
    }
    if let TabCommand::Agent(agent) = &spec.command {
        spec.title = crate::service::spawn_title::normalize_agent_title(spec.title.take());
        crate::service::spawn_title::validate_agent_tab_title(
            spec.title.as_deref(),
            agent.agent_name.as_deref(),
        )?;
    } else if let Some(title) = spec.title.as_deref() {
        crate::service::tab::validate_tab_title(title)?;
    }
    if spec.cmd.is_some() && !matches!(spec.command, TabCommand::Shell) {
        return Err("utility commands require PTY shell backing".into());
    }
    spec.icon = crate::service::utility_tabs::normalize_icon(spec.icon.take())?;
    if spec.icon.is_some() && !matches!(spec.command, TabCommand::Shell) {
        return Err("utility icons require PTY shell backing".into());
    }
    spec.description =
        crate::service::tab::normalize_optional_tab_description(spec.description.take())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_title_must_be_one_word_and_description_keeps_its_text() {
        let mut spec = SpawnTabSpec {
            shell: None,
            title: Some("Scout".into()),
            description: Some("Reviews plans and records task progress".into()),
            ..Default::default()
        };

        prepare_tab_identity(&mut spec).unwrap();

        assert_eq!(spec.title.as_deref(), Some("Scout"));
        assert_eq!(
            spec.description.as_deref(),
            Some("Reviews plans and records task progress")
        );
    }

    #[test]
    fn spawn_rejects_multiword_title() {
        let mut spec = SpawnTabSpec {
            shell: None,
            title: Some("Review plans".into()),
            ..Default::default()
        };

        let error = prepare_tab_identity(&mut spec).unwrap_err();

        assert!(error.contains("one word"));
        assert!(error.contains("description"));
    }
}

#[cfg(test)]
#[path = "spawn_tab_identity/purpose_tests.rs"]
mod purpose_tests;
