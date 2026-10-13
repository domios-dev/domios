use crate::semantic_marker::SemanticEvent;
use codeterm_pty::multiplexer::types::{ShellActivity, ShellCommandState};

#[derive(Default)]
pub struct ShellCommandStateModel {
    state: ShellCommandState,
    authenticated: bool,
    rejected: bool,
}

impl ShellCommandStateModel {
    pub fn state(&self) -> ShellCommandState {
        self.state
    }

    pub fn apply(&mut self, event: &SemanticEvent, expected_nonce: Option<&str>) {
        match event {
            SemanticEvent::CommandLine { nonce, .. } => {
                self.authenticated =
                    expected_nonce.is_none_or(|expected| nonce.as_deref() == Some(expected));
                self.rejected = !self.authenticated;
            }
            SemanticEvent::PromptStart => {
                if !self.rejected && self.state.activity == ShellActivity::Unknown {
                    self.state.activity = ShellActivity::Idle;
                }
                self.authenticated = false;
                self.rejected = false;
            }
            SemanticEvent::CommandStart if self.accepts_cycle(expected_nonce) => {
                self.state.activity = ShellActivity::Running;
            }
            SemanticEvent::CommandEnd { exit_code } if self.accepts_cycle(expected_nonce) => {
                self.state.activity = ShellActivity::Idle;
                self.state.last_exit_code = *exit_code;
                self.authenticated = false;
            }
            _ => {}
        }
    }

    fn accepts_cycle(&self, expected_nonce: Option<&str>) -> bool {
        !self.rejected && (expected_nonce.is_none() || self.authenticated)
    }
}

#[cfg(test)]
mod tests;
