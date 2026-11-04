use codeterm_types::agent::keys::action_key_bytes;

#[derive(Default)]
pub struct InputMode {
    parser: vte::Parser,
    state: ModeState,
}

#[derive(Default)]
struct ModeState {
    win32: bool,
    bracketed_paste: bool,
    mouse: u16,
    sgr_mouse: bool,
    urxvt_mouse: bool,
    focus: bool,
}

impl vte::Perform for ModeState {
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        if intermediates == b"!" && action == 'p' {
            self.reset_application_modes();
        }
        if intermediates != b"?" || !matches!(action, 'h' | 'l') {
            return;
        }
        let enabled = action == 'h';
        for param in params.iter() {
            match param {
                [9001] => self.win32 = enabled,
                [2004] => self.bracketed_paste = enabled,
                [9 | 1000 | 1002 | 1003] => self.mouse = if enabled { param[0] } else { 0 },
                [1004] => self.focus = enabled,
                [1006] => self.sgr_mouse = enabled,
                [1015] => self.urxvt_mouse = enabled,
                _ => {}
            }
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if !ignore && intermediates.is_empty() && byte == b'c' {
            *self = Self::default();
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _: bool) {
        if params
            .first()
            .is_some_and(|param| matches!(*param, b"133" | b"633"))
            && params.get(1) == Some(&b"A".as_slice())
        {
            self.reset_application_modes();
        }
    }
}

impl ModeState {
    fn reset_application_modes(&mut self) {
        let win32 = self.win32;
        *self = Self {
            win32,
            ..Self::default()
        };
    }
}

impl InputMode {
    pub fn process(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.state, data);
    }

    pub fn win32(&self) -> bool {
        self.state.win32
    }

    pub fn bracketed_paste(&self) -> bool {
        self.state.bracketed_paste
    }

    pub fn reset_application_modes(&mut self) {
        self.state.reset_application_modes();
    }

    pub fn filter_terminal_reports(&self, data: &[u8]) -> Vec<u8> {
        let mut output = Vec::with_capacity(data.len());
        let mut remaining = data;
        while !remaining.is_empty() {
            if let Some(body) = remaining.strip_prefix(b"\x1b[200~") {
                if let Some(end) = body.windows(6).position(|window| window == b"\x1b[201~") {
                    if self.state.bracketed_paste {
                        output.extend_from_slice(&remaining[..6 + end + 6]);
                    } else {
                        output.extend_from_slice(&body[..end]);
                    }
                    remaining = &body[end + 6..];
                    continue;
                }
            }
            if let Some((length, allowed)) = self.terminal_report(remaining) {
                if allowed {
                    output.extend_from_slice(&remaining[..length]);
                }
                remaining = &remaining[length..];
            } else {
                output.push(remaining[0]);
                remaining = &remaining[1..];
            }
        }
        output
    }

    fn terminal_report(&self, data: &[u8]) -> Option<(usize, bool)> {
        let csi = data.strip_prefix(b"\x1b[")?;
        if matches!(csi.first(), Some(b'I' | b'O')) {
            return Some((3, self.state.focus));
        }
        if csi.starts_with(b"M") && csi.len() >= 4 {
            return Some((
                6,
                self.mouse_event_allowed(csi[1].saturating_sub(32).into())
                    && !self.state.sgr_mouse
                    && !self.state.urxvt_mouse,
            ));
        }
        let (params, sgr) = csi
            .strip_prefix(b"<")
            .map_or((csi, false), |params| (params, true));
        let end = params
            .iter()
            .position(|byte| !byte.is_ascii_digit() && *byte != b';')?;
        if !matches!(params[end], b'M' | b'm')
            || params[..end].iter().filter(|byte| **byte == b';').count() != 2
        {
            return None;
        }
        let encoding_enabled = if sgr {
            self.state.sgr_mouse
        } else {
            self.state.urxvt_mouse
        };
        let button = std::str::from_utf8(params.split(|byte| *byte == b';').next()?)
            .ok()?
            .parse::<u16>()
            .ok()?;
        let button = if sgr {
            button
        } else {
            button.saturating_sub(32)
        };
        Some((
            2 + usize::from(sgr) + end + 1,
            self.mouse_event_allowed(button) && encoding_enabled,
        ))
    }

    fn mouse_event_allowed(&self, button: u16) -> bool {
        match self.state.mouse {
            9 => button & 3 != 3 && button & 32 == 0,
            1000 => button & 32 == 0,
            1002 => button & 32 == 0 || button & 3 != 3,
            1003 => true,
            _ => false,
        }
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
