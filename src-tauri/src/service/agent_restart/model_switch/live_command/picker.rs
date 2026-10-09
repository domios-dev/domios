use super::*;
use crate::agent::provider::{ModelSwitchPicker, ModelSwitchPickerPosition};

pub(super) struct CommandPicker {
    screen: Regex,
    selected: Regex,
    target: Regex,
    declaration: ModelSwitchPicker,
    last_position: Option<usize>,
    submitted: bool,
    steps: usize,
    injected: bool,
    started: Instant,
    target_label: String,
    tab_id: String,
}

impl CommandPicker {
    pub(super) fn new(
        declaration: &ModelSwitchPicker,
        target: &CommandTarget,
        effort: Option<&str>,
    ) -> Result<Self, String> {
        let pattern = |template: &str| {
            confirmation_pattern(template, &target.model, &target.display_name, effort)
        };
        let screen = pattern(&declaration.screen_regex)?;
        let selected = pattern(&declaration.selected_regex)?;
        let target_pattern = pattern(&declaration.target_regex)?;
        if screen.is_match("")
            || !selected
                .capture_names()
                .flatten()
                .any(|name| name == "position")
            || !target_pattern
                .capture_names()
                .flatten()
                .any(|name| name == "position")
            || [
                &declaration.previous_key,
                &declaration.next_key,
                &declaration.response_key,
                &declaration.cancel_key,
            ]
            .iter()
            .any(|key| key.is_empty() || key.len() > 64)
        {
            return Err("invalid live command picker declaration".into());
        }
        Ok(Self {
            screen,
            selected,
            target: target_pattern,
            declaration: declaration.clone(),
            last_position: None,
            submitted: false,
            steps: 0,
            injected: false,
            started: Instant::now(),
            target_label: effort.unwrap_or(&target.display_name).to_owned(),
            tab_id: target.tab_id.clone(),
        })
    }

    pub(super) fn present(&self, screen: &str) -> bool {
        self.screen.is_match(screen)
    }

    fn frame<'a>(&self, screen: &'a str) -> Option<&'a str> {
        let mut offset = 0;
        let mut latest = None;
        while let Some(frame) = self.screen.find_at(screen, offset) {
            latest = Some(frame.as_str());
            offset = frame.start() + screen[frame.start()..].chars().next()?.len_utf8();
        }
        latest
    }

    pub(super) fn injected(&mut self) {
        self.injected = true;
    }

    fn position(&self, captures: &regex::Captures<'_>) -> Option<usize> {
        let value = captures.name("position")?.as_str();
        match self.declaration.position {
            ModelSwitchPickerPosition::Number => value.parse().ok(),
            ModelSwitchPickerPosition::Column => Some(value.chars().count()),
        }
    }

    pub(super) fn respond(
        &mut self,
        screen: &str,
        write: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.injected || self.submitted {
            return Ok(());
        }
        let Some(screen) = self.frame(screen) else {
            return Ok(());
        };
        let Some(selected) = self.selected.captures_iter(screen).last() else {
            return Ok(());
        };
        let target = self
            .target
            .captures_iter(screen)
            .find(|capture| {
                capture
                    .name("label")
                    .is_some_and(|label| label.as_str().eq_ignore_ascii_case(&self.target_label))
            })
            .or_else(|| self.target.captures(screen));
        let Some(target) = target else {
            return Err("the requested selection is absent from the live picker; restart and resume is available as an explicit fallback".into());
        };
        let current = self
            .position(&selected)
            .ok_or("invalid picker cursor position")?;
        let desired = self
            .position(&target)
            .ok_or("invalid picker target position")?;
        let end = match self.declaration.position {
            ModelSwitchPickerPosition::Number => desired,
            ModelSwitchPickerPosition::Column => {
                desired
                    + target
                        .name("label")
                        .ok_or("missing picker target label")?
                        .as_str()
                        .chars()
                        .count()
                        .saturating_sub(1)
            }
        };
        if self.last_position == Some(current) {
            return Ok(());
        }
        if self.steps >= 32 {
            return Err("the live picker did not reach the requested selection".into());
        }
        self.last_position = Some(current);
        self.steps += 1;
        let key = if current < desired {
            &self.declaration.next_key
        } else if current > end {
            &self.declaration.previous_key
        } else {
            self.submitted = true;
            &self.declaration.response_key
        };
        log::info!(
            "[agent-model/timing] tab={} phase=picker_step current={} desired={} submit={} elapsed_ms={}",
            self.tab_id,
            current,
            desired,
            self.submitted,
            self.started.elapsed().as_millis()
        );
        write(key.as_bytes())
    }

    pub(super) fn cancel_key(&self) -> &[u8] {
        self.declaration.cancel_key.as_bytes()
    }
}

impl ServiceContext {
    pub(super) fn respond_to_command_picker(
        &self,
        target: &CommandTarget,
        picker: &mut CommandPicker,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        if !mux
            .get_tab(&target.tab_id)
            .is_some_and(|tab| target.matches(tab))
        {
            return Err("the session changed before the live picker response".into());
        }
        let screen_state = self.screen_state.lock();
        let screen = screen_state
            .get_screen_text(&target.pty_id)
            .unwrap_or_default();
        picker.respond(&screen, |keys| self.pty.lock().write(&target.pty_id, keys))
    }

    pub(super) fn cancel_command_picker(
        &self,
        target: &CommandTarget,
        picker: &CommandPicker,
    ) -> Result<(), String> {
        let declaration = crate::agent::provider::ModelSwitchPrompt {
            screen_regex: picker.screen.as_str().into(),
            response_keys: vec![picker.declaration.response_key.clone()],
            cancel_keys: vec![String::from_utf8_lossy(picker.cancel_key()).into_owned()],
        };
        let mut prompt = CommandPrompt::new(&declaration, target, None)?;
        prompt.injected();
        self.cancel_command_prompt(target, &mut prompt)
    }
}

#[cfg(test)]
mod tests;
