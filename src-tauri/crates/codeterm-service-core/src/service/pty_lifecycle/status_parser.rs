pub(super) const STATUS_REPLY: &[u8] = b"\x1b[0n";

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Escape,
    EscapeIntermediate,
    CsiEntry,
    CsiParam,
    CsiIntermediate,
    CsiIgnore,
    Osc,
    Dcs,
    Sos,
}

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct StatusParser {
    state: State,
    first_param: u8,
    first_param_done: bool,
    collected: bool,
    utf8_value: u32,
    utf8_min: u32,
    utf8_remaining: u8,
}

impl StatusParser {
    pub(super) fn feed(&mut self, data: &[u8], mut reply: impl FnMut(&'static [u8])) {
        for &byte in data {
            if self.utf8_remaining != 0 {
                if byte & 0xc0 == 0x80 {
                    self.utf8_value = (self.utf8_value << 6) | u32::from(byte & 0x3f);
                    self.utf8_remaining -= 1;
                    if self.utf8_remaining == 0 {
                        let value = self.utf8_value;
                        if value >= self.utf8_min
                            && value <= 0x10ffff
                            && !(0xd800..=0xdfff).contains(&value)
                            && value != 0xfeff
                        {
                            self.codepoint(value, &mut reply);
                        }
                    }
                    continue;
                }
                self.utf8_remaining = 0;
            }
            match byte {
                0x00..=0x7f => self.codepoint(u32::from(byte), &mut reply),
                0xc0..=0xdf => self.start_utf8(byte & 0x1f, 1, 0x80),
                0xe0..=0xef => self.start_utf8(byte & 0x0f, 2, 0x800),
                0xf0..=0xf7 => self.start_utf8(byte & 0x07, 3, 0x10000),
                _ => {}
            }
        }
    }

    fn start_utf8(&mut self, value: u8, remaining: u8, min: u32) {
        self.utf8_value = u32::from(value);
        self.utf8_remaining = remaining;
        self.utf8_min = min;
    }

    fn start_csi(&mut self) {
        self.state = State::CsiEntry;
        self.first_param = 0;
        self.first_param_done = false;
        self.collected = false;
    }

    fn codepoint(&mut self, code: u32, reply: &mut impl FnMut(&'static [u8])) {
        use State::*;
        match code {
            0x1b => self.state = Escape,
            0x9b => self.start_csi(),
            0x90 => self.state = Dcs,
            0x9d => self.state = Osc,
            0x98 | 0x9e | 0x9f => self.state = Sos,
            0x18 | 0x1a | 0x80..=0x97 | 0x99 | 0x9a | 0x9c => self.state = Ground,
            _ => match self.state {
                Ground => {}
                Escape => match code {
                    0x5b => self.start_csi(),
                    0x5d => self.state = Osc,
                    0x50 => self.state = Dcs,
                    0x58 | 0x5e | 0x5f => self.state = Sos,
                    0x00..=0x1f | 0x7f => {}
                    0x20..=0x2f => self.state = EscapeIntermediate,
                    _ => self.state = Ground,
                },
                EscapeIntermediate => match code {
                    0x00..=0x2f | 0x7f => {}
                    _ => self.state = Ground,
                },
                Osc => {
                    if code == 7 {
                        self.state = Ground;
                    }
                }
                Dcs => {}
                Sos => {
                    if code >= 0xa0 {
                        self.state = Ground;
                    }
                }
                CsiEntry | CsiParam => match code {
                    0x00..=0x1f | 0x7f => {}
                    0x30..=0x39 => {
                        if !self.first_param_done {
                            self.first_param = (self.first_param * 10 + (code as u8 - b'0')).min(6);
                        }
                        self.state = CsiParam;
                    }
                    0x3a | 0x3b => {
                        self.first_param_done = true;
                        self.state = CsiParam;
                    }
                    0x3c..=0x3f => {
                        if matches!(self.state, CsiEntry) {
                            self.collected = true;
                            self.state = CsiParam;
                        } else {
                            self.state = CsiIgnore;
                        }
                    }
                    0x20..=0x2f => self.state = CsiIntermediate,
                    0x40..=0x7e => {
                        if code == u32::from(b'n') && self.first_param == 5 && !self.collected {
                            reply(STATUS_REPLY);
                        }
                        self.state = Ground;
                    }
                    _ => self.state = Ground,
                },
                CsiIntermediate => match code {
                    0x00..=0x2f | 0x7f => {}
                    0x30..=0x3f => self.state = CsiIgnore,
                    _ => self.state = Ground,
                },
                CsiIgnore => match code {
                    0x00..=0x3f | 0x7f | 0xa0.. => {}
                    _ => self.state = Ground,
                },
            },
        }
    }
}
