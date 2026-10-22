use crate::service::ServiceContext;
use codeterm_pty::pty::input_mode::encode_action_key;

impl ServiceContext {
    pub fn encode_pty_action_key(&self, tab_id: &str, key: &str) -> Option<Vec<u8>> {
        let win32 = self
            .resolve_pty_id(tab_id)
            .ok()
            .is_some_and(|pty_id| self.screen_state.win32_input_mode(&pty_id));
        encode_action_key(key, cfg!(windows), win32)
    }

    pub fn encode_provider_control_key(&self, pty_id: &str, frame: &[u8]) -> Option<Vec<u8>> {
        if !cfg!(windows) || frame != b"\x03" {
            return None;
        }
        encode_action_key("ctrl-c", true, self.screen_state.win32_input_mode(pty_id))
    }
}

#[cfg(test)]
mod tests;
