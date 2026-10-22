use super::*;

pub(super) struct CommandPrompt {
    pattern: Regex,
    keys: Vec<u8>,
    cancel_keys: Vec<u8>,
    cancelled: bool,
    injected: bool,
    answered: bool,
}

impl CommandPrompt {
    pub(super) fn new(
        declaration: &crate::agent::provider::ModelSwitchPrompt,
        target: &CommandTarget,
        effort: Option<&str>,
    ) -> Result<Self, String> {
        let pattern = confirmation_pattern(
            &declaration.screen_regex,
            &target.model,
            &target.display_name,
            effort,
        )?;
        let keys = declaration.response_keys.concat().into_bytes();
        let cancel_keys = declaration.cancel_keys.concat().into_bytes();
        if cancel_keys.len() > 64 || declaration.cancel_keys.iter().any(String::is_empty) {
            return Err("invalid live command cancellation declaration".into());
        }
        if pattern.is_match("")
            || keys.is_empty()
            || keys.len() > 64
            || declaration.response_keys.iter().any(String::is_empty)
        {
            return Err("invalid live command prompt declaration".into());
        }
        Ok(Self {
            pattern,
            keys,
            cancel_keys,
            cancelled: false,
            injected: false,
            answered: false,
        })
    }

    pub(super) fn present(&self, screen: &str) -> bool {
        self.pattern.is_match(screen)
    }

    pub(super) fn injected(&mut self) {
        self.injected = true;
    }

    pub(super) fn answered(&self) -> bool {
        self.answered
    }

    pub(super) fn cancel(
        &mut self,
        screen: &str,
        write: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<bool, String> {
        if !self.injected || !self.present(screen) {
            return Ok(true);
        }
        if self.cancel_keys.is_empty() {
            return Err(
                "the live command dialog remains open and declares no cancellation keys".into(),
            );
        }
        if !self.cancelled {
            self.cancelled = true;
            write(&self.cancel_keys)?;
        }
        Ok(false)
    }

    pub(super) fn respond(
        &mut self,
        screen: &str,
        write: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.injected || self.answered || self.cancelled || !self.present(screen) {
            return Ok(());
        }
        self.answered = true;
        write(&self.keys).map_err(|error| format!("the live command confirmation could not be answered: {error}; restart and resume is available as an explicit fallback"))
    }
}

impl ServiceContext {
    pub(super) fn command_input_restored(&self, target: &CommandTarget) -> bool {
        let Some(readiness) = &target.readiness else {
            return false;
        };
        let Some(screens) = self.screen_state.existing(&target.pty_id) else {
            return false;
        };
        let screens = screens.lock();
        screens
            .get_screen_rows()
            .zip(screens.composer_input_state())
            .is_some_and(|(rows, input)| readiness.matches(&rows.join("\n"), &input))
    }

    pub(super) fn verify_command_input_restored(
        &self,
        target: &CommandTarget,
    ) -> Result<(), String> {
        let signal = command_signal(&self.screen_state, &target.pty_id);
        let deadline = Instant::now() + Duration::from_secs(3);
        let result = (|| loop {
            let generation = signal.generation();
            if !target.current(self) {
                return Err("the session changed before normal command input was restored".into());
            }
            if self.command_input_restored(target) {
                return Ok(());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(
                    "normal command input was not observed after dialog cancellation".into(),
                );
            };
            if !signal.wait_after(generation, remaining) {
                return Err(
                    "normal command input was not observed after dialog cancellation".into(),
                );
            }
        })();
        self.screen_state
            .unsubscribe_screen_changes(&target.pty_id, &signal);
        result
    }

    pub(super) fn respond_to_command_prompt(
        &self,
        target: &CommandTarget,
        prompt: &mut CommandPrompt,
    ) -> Result<(), String> {
        let mux = self.mux.read();
        if !mux
            .get_tab(&target.tab_id)
            .is_some_and(|tab| target.matches(tab))
        {
            return Err("the session changed before the live command confirmation".into());
        }
        let screen_state = self
            .screen_state
            .existing(&target.pty_id)
            .ok_or_else(|| "the command screen is unavailable".to_owned())?;
        let screen_state = screen_state.lock();
        let screen = screen_state.get_screen_text().unwrap_or_default();
        prompt.respond(&screen, |keys| self.pty.lock().write(&target.pty_id, keys))
    }

    pub(super) fn cancel_command_prompt(
        &self,
        target: &CommandTarget,
        prompt: &mut CommandPrompt,
    ) -> Result<(), String> {
        let signal = command_signal(&self.screen_state, &target.pty_id);
        let deadline = Instant::now() + Duration::from_secs(3);
        let result = (|| loop {
            let generation = signal.generation();
            let cleared = {
                let mux = self.mux.read();
                if !mux
                    .get_tab(&target.tab_id)
                    .is_some_and(|tab| target.matches(tab))
                {
                    return Err("session changed; the old command dialog was not touched".into());
                }
                let screen_state = self
                    .screen_state
                    .existing(&target.pty_id)
                    .ok_or_else(|| "the command screen is unavailable".to_owned())?;
                let screen_state = screen_state.lock();
                let screen = screen_state
                    .get_screen_text()
                    .ok_or_else(|| "cannot verify the live command dialog cleared".to_owned())?;
                prompt.cancel(&screen, |keys| self.pty.lock().write(&target.pty_id, keys))?
            };
            if cleared {
                return Ok(());
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err("the live command dialog did not clear after cancellation".into());
            };
            signal.wait_after(generation, remaining);
        })();
        self.screen_state
            .unsubscribe_screen_changes(&target.pty_id, &signal);
        result
    }
}
