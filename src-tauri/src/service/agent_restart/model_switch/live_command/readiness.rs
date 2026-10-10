use super::*;
use codeterm_pty::pty::screen_state::ComposerInputState;
use std::sync::Arc;

pub(super) struct CommandReadiness {
    input: Regex,
    dialogs: Vec<Regex>,
}

impl CommandReadiness {
    pub(super) fn from_declaration(
        commands: &crate::agent::provider::ModelSwitchCommands,
        target: &CommandTarget,
    ) -> Result<Option<Arc<Self>>, ModelSwitchError> {
        let Some(pattern) = &commands.ready_input_regex else {
            return Ok(None);
        };
        let input = Regex::new(pattern)
            .map_err(|error| ModelSwitchError::LiveCommandFailed(error.to_string()))?;
        if input.is_match("") {
            return Err(ModelSwitchError::LiveCommandFailed(
                "empty ready-input declaration".into(),
            ));
        }
        let mut dialogs = Vec::new();
        for command in std::iter::once(&commands.model).chain(commands.effort.iter()) {
            for pattern in command
                .prompt
                .as_ref()
                .map(|prompt| &prompt.screen_regex)
                .into_iter()
                .chain(command.picker.as_ref().map(|picker| &picker.screen_regex))
            {
                dialogs.push(
                    confirmation_pattern(pattern, &target.model, &target.display_name, None)
                        .map_err(ModelSwitchError::LiveCommandFailed)?,
                );
            }
        }
        Ok(Some(Arc::new(Self { input, dialogs })))
    }

    pub(super) fn matches(&self, screen: &str, input: &ComposerInputState) -> bool {
        let cursor = input
            .cursor_visible
            .then_some(input.cursor_row)
            .or(input.inverse_caret_row);
        !self.dialogs.iter().any(|dialog| dialog.is_match(screen))
            && cursor
                .and_then(|row| screen.lines().nth(row as usize))
                .is_some_and(|row| self.input.is_match(row))
    }
}

pub(super) fn input_ready(
    readiness: Option<&CommandReadiness>,
    frame: Option<(&str, &ComposerInputState)>,
    status_idle: bool,
) -> bool {
    readiness.map_or(status_idle, |readiness| {
        frame.is_some_and(|(screen, input)| readiness.matches(screen, input))
    })
}

#[cfg(test)]
mod tests;
