use super::{SpawnTabSpec, TabCommand};

pub(super) fn prepare_tab_identity(spec: &mut SpawnTabSpec) -> Result<(), String> {
    if let TabCommand::Agent(agent) = &spec.command {
        spec.title = super::spawn_title::normalize_agent_title(spec.title.take());
        super::spawn_title::validate_agent_tab_title(
            spec.title.as_deref(),
            agent.agent_name.as_deref(),
        )?;
    } else if let Some(title) = spec.title.as_deref() {
        super::tab::validate_tab_title(title)?;
    }
    spec.description = super::tab::normalize_optional_tab_description(spec.description.take())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_title_must_be_one_word_and_description_keeps_its_text() {
        let mut spec = SpawnTabSpec {
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
            title: Some("Review plans".into()),
            ..Default::default()
        };

        let error = prepare_tab_identity(&mut spec).unwrap_err();

        assert!(error.contains("one word"));
        assert!(error.contains("description"));
    }
}
