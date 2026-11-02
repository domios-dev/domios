#[derive(Default)]
pub(super) struct TerminalQueries {
    state: State,
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    Csi {
        first: u32,
        first_complete: bool,
        valid: bool,
    },
    String {
        osc: bool,
    },
}

impl TerminalQueries {
    pub(super) fn replies(&mut self, bytes: &[u8]) -> Vec<&'static [u8]> {
        let mut replies = Vec::new();
        for &byte in bytes {
            if byte == 0x1b {
                self.state = State::Escape;
                continue;
            }
            if matches!(byte, 0x18 | 0x1a) {
                self.state = State::Ground;
                continue;
            }
            match &mut self.state {
                State::Ground => {}
                State::Escape => match byte {
                    b'[' => {
                        self.state = State::Csi {
                            first: 0,
                            first_complete: false,
                            valid: true,
                        };
                    }
                    b']' | b'P' | b'X' | b'^' | b'_' => {
                        self.state = State::String { osc: byte == b']' };
                    }
                    0x00..=0x1f | 0x7f => {}
                    _ => self.state = State::Ground,
                },
                State::String { osc } => {
                    if *osc && byte == 0x07 {
                        self.state = State::Ground;
                    }
                }
                State::Csi {
                    first,
                    first_complete,
                    valid,
                } => match byte {
                    b'0'..=b'9' => {
                        if !*first_complete {
                            *first = first
                                .saturating_mul(10)
                                .saturating_add(u32::from(byte - b'0'));
                        }
                    }
                    b';' | b':' => *first_complete = true,
                    0x00..=0x1f | 0x7f => {}
                    0x40..=0x7e => {
                        if *valid {
                            let reply: Option<&'static [u8]> = match (byte, *first) {
                                (b'c', 0) => Some(b"\x1b[?1;2c"),
                                (b'n', 5) => Some(b"\x1b[0n"),
                                (b'n', 6) => Some(b"\x1b[1;1R"),
                                _ => None,
                            };
                            replies.extend(reply);
                        }
                        self.state = State::Ground;
                    }
                    _ => *valid = false,
                },
            }
        }
        replies
    }
}

#[cfg(test)]
mod tests;
