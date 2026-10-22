use codeterm_types::agent::keys::action_key_bytes;

#[derive(Default)]
pub struct InputMode {
    parser: vte::Parser,
    state: ModeState,
}

#[derive(Default)]
struct ModeState {
    win32: bool,
}

impl vte::Perform for ModeState {
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if !ignore
            && intermediates == b"?"
            && matches!(action, 'h' | 'l')
            && params.iter().any(|param| param == [9001])
        {
            self.win32 = action == 'h';
        }
    }
}

impl InputMode {
    pub fn process(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.state, data);
    }

    pub fn win32(&self) -> bool {
        self.state.win32
    }
}

pub fn encode_action_key(key: &str, windows: bool, win32: bool) -> Option<Vec<u8>> {
    let bytes = action_key_bytes(key)?;
    if !windows || !win32 || key.chars().count() == 1 {
        return Some(bytes);
    }
    let normalized = key.replace('_', "-").to_ascii_lowercase();
    let ctrl = normalized
        .strip_prefix("ctrl-")
        .or_else(|| normalized.strip_prefix("control-"))
        .or_else(|| normalized.strip_prefix("c-"));
    let (vk, unicode, modifiers) = if let Some(ctrl) = ctrl {
        (
            if ctrl == "[" {
                219
            } else {
                u16::from(ctrl.as_bytes()[0].to_ascii_uppercase())
            },
            // PSReadLine remaps control characters through the active keyboard layout.
            u16::from(ctrl.as_bytes()[0]),
            8,
        )
    } else {
        match normalized.as_str() {
            "enter" | "return" => (13, 13, 0),
            "esc" | "escape" => (27, 27, 0),
            "tab" => (9, 9, 0),
            "backspace" | "bs" => (8, 8, 0),
            "delete" | "del" => (46, 0, 256),
            "up" | "arrow-up" => (38, 0, 256),
            "down" | "arrow-down" => (40, 0, 256),
            "right" | "arrow-right" => (39, 0, 256),
            "left" | "arrow-left" => (37, 0, 256),
            "home" => (36, 0, 256),
            "end" => (35, 0, 256),
            "pageup" | "page-up" | "pgup" => (33, 0, 256),
            "pagedown" | "page-down" | "pgdn" => (34, 0, 256),
            _ => return Some(bytes),
        }
    };
    Some(
        format!("\x1b[{vk};0;{unicode};1;{modifiers};1_\x1b[{vk};0;{unicode};0;{modifiers};1_")
            .into_bytes(),
    )
}

#[cfg(test)]
mod tests;
