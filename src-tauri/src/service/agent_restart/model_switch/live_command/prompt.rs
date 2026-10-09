use super::*;

pub(super) struct CommandPrompt {
    pattern: Regex,
    keys: Vec<u8>,
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

    pub(super) fn respond(
        &mut self,
        screen: &str,
        write: impl FnOnce(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if !self.injected || self.answered || !self.present(screen) {
            return Ok(());
        }
        self.answered = true;
        write(&self.keys).map_err(|error| format!("the live command confirmation could not be answered: {error}; restart and resume is available as an explicit fallback"))
    }
}

impl ServiceContext {
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
        let screen_state = self.screen_state.lock();
        let screen = screen_state
            .get_screen_text(&target.pty_id)
            .unwrap_or_default();
        prompt.respond(&screen, |keys| self.pty.lock().write(&target.pty_id, keys))
    }
}
