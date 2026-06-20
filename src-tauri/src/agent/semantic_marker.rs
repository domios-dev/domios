//! Provider-agnostic streaming parser for shell-integration semantic markers.
//!
//! Pulls OSC 133 / OSC 633 (VS Code superset) command-lifecycle sequences and
//! OSC 7 cwd updates out of the raw PTY byte stream into typed `SemanticEvent`s.
//! Pure and deterministic; knows no provider and matches no vendor string. Sits
//! beside the `SignalExtractor` in the core extraction layer — "core extracts
//! generic signals, consumers interpret".

/// A typed semantic marker extracted from the PTY stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticEvent {
    /// 133/633 `A` — prompt is about to be drawn.
    PromptStart,
    /// 133/633 `B` — prompt drawn, command input begins.
    PromptEnd,
    /// 133/633 `C` — command output begins.
    CommandStart,
    /// 133/633 `D[;<code>]` — command finished, with optional exit code.
    CommandEnd { exit_code: Option<i32> },
    /// 633 `E;<cmd>[;<nonce>]` — the command line text, with optional session
    /// nonce. The nonce lets a consumer reject text whose nonce mismatches the
    /// session nonce (anti output-spoofing). `cmd` is the raw escaped segment as
    /// emitted (VS Code escapes literal `;`), so the first `;` always delimits
    /// the nonce.
    CommandLine { text: String, nonce: Option<String> },
    /// 633 `P;Key=Value` — a per-command property (e.g. `Cwd`).
    Property { key: String, value: String },
    /// OSC 7 — current working directory.
    Cwd { path: String },
}

/// Cap on a single OSC payload so a stray unterminated `ESC ]` in binary output
/// cannot grow the carry buffer without bound.
const MAX_OSC_PAYLOAD: usize = 8192;

#[derive(Default, Clone, Copy)]
enum State {
    #[default]
    Normal,
    Esc,
    InOsc,
    OscEsc,
}

/// Streaming OSC marker extractor. Carries its scan state and partial payload so
/// a sequence split across PTY read chunks is reassembled on the next `feed`.
#[derive(Default)]
pub struct SemanticMarkerParser {
    state: State,
    buf: Vec<u8>,
}

impl SemanticMarkerParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the next raw PTY chunk; returns the markers completed within it.
    /// Non-OSC bytes flow on untouched (this parser only observes, never mutates
    /// the stream) and produce no events.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SemanticEvent> {
        let mut out = Vec::new();
        for &b in chunk {
            match self.state {
                State::Normal => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    }
                }
                State::Esc => {
                    if b == b']' {
                        self.state = State::InOsc;
                        self.buf.clear();
                    } else if b == 0x1b {
                        // consecutive ESC — stay armed for the introducer
                    } else {
                        self.state = State::Normal;
                    }
                }
                State::InOsc => {
                    if b == 0x07 {
                        if let Some(ev) = parse_osc(&self.buf) {
                            out.push(ev);
                        }
                        self.reset();
                    } else if b == 0x1b {
                        self.state = State::OscEsc;
                    } else if self.buf.len() >= MAX_OSC_PAYLOAD {
                        self.reset();
                    } else {
                        self.buf.push(b);
                    }
                }
                State::OscEsc => {
                    if b == b'\\' {
                        if let Some(ev) = parse_osc(&self.buf) {
                            out.push(ev);
                        }
                        self.reset();
                    } else {
                        // ESC not followed by `\` aborts the OSC; reprocess the
                        // byte so a fresh ESC still arms the next introducer.
                        self.reset();
                        if b == 0x1b {
                            self.state = State::Esc;
                        }
                    }
                }
            }
        }
        out
    }

    fn reset(&mut self) {
        self.state = State::Normal;
        self.buf.clear();
    }
}

/// Parse a complete OSC payload (introducer and terminator already stripped).
fn parse_osc(payload: &[u8]) -> Option<SemanticEvent> {
    let s = String::from_utf8_lossy(payload);
    let (code, rest) = match s.split_once(';') {
        Some((c, r)) => (c, r),
        None => (s.as_ref(), ""),
    };
    match code {
        "7" => parse_osc7(rest).map(|path| SemanticEvent::Cwd { path }),
        "133" | "633" => parse_shell_integration(rest),
        // ConEmu/Windows-friendly cwd: `OSC 9 ; 9 ; <cwd> ST` (raw path, no nonce).
        "9" => rest
            .strip_prefix("9;")
            .filter(|cwd| !cwd.is_empty())
            .map(|cwd| SemanticEvent::Cwd {
                path: cwd.to_string(),
            }),
        // iTerm2 cwd: `OSC 1337 ; CurrentDir=<cwd> ST` (raw path, no nonce).
        "1337" => rest
            .strip_prefix("CurrentDir=")
            .filter(|cwd| !cwd.is_empty())
            .map(|cwd| SemanticEvent::Cwd {
                path: cwd.to_string(),
            }),
        _ => None,
    }
}

fn parse_shell_integration(rest: &str) -> Option<SemanticEvent> {
    let (sub, args) = match rest.split_once(';') {
        Some((s, a)) => (s, Some(a)),
        None => (rest, None),
    };
    match sub {
        "A" => Some(SemanticEvent::PromptStart),
        "B" => Some(SemanticEvent::PromptEnd),
        "C" => Some(SemanticEvent::CommandStart),
        "D" => Some(SemanticEvent::CommandEnd {
            exit_code: args.and_then(|a| a.trim().parse::<i32>().ok()),
        }),
        "E" => {
            let a = args.unwrap_or("");
            let (text, nonce) = match a.split_once(';') {
                Some((t, n)) => (t.to_string(), Some(n.to_string())),
                None => (a.to_string(), None),
            };
            Some(SemanticEvent::CommandLine { text, nonce })
        }
        "P" => {
            let (key, value) = args.unwrap_or("").split_once('=')?;
            Some(SemanticEvent::Property {
                key: key.to_string(),
                value: value.to_string(),
            })
        }
        _ => None,
    }
}

/// Extract the cwd path from an OSC 7 payload (`file://<host>/<path>` or a bare
/// path), mirroring the frontend handler: percent-decode, then strip the leading
/// slash off a Windows drive path (`/C:/...` -> `C:/...`).
fn parse_osc7(payload: &str) -> Option<String> {
    let raw = match payload.strip_prefix("file://") {
        Some(after_scheme) => match after_scheme.find('/') {
            Some(idx) => &after_scheme[idx..],
            None => "",
        },
        None => payload,
    };
    if raw.is_empty() {
        return None;
    }
    let mut path = percent_decode(raw);
    if is_windows_drive_path(&path) {
        path.remove(0);
    }
    Some(path)
}

fn is_windows_drive_path(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':'
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;
