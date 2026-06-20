//! Capability accumulators — fold `SemanticEvent`s into shell command-lifecycle
//! and cwd state. Mirrors VS Code's CommandDetection/CwdDetection split: the
//! backend reader feeds events, consumers read the CAPABILITIES (command list,
//! cwd, lifecycle stream) — never the raw buffer. Pure and deterministic.

use std::collections::HashMap;

use serde::Serialize;

use crate::agent::semantic_marker::{SemanticEvent, SemanticMarkerParser};

/// One detected shell command. Built incrementally from prompt/command/finished
/// marks; finalized (pushed to the list) at `CommandEnd`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellCommand {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Pre-exec `C` (output start) was observed. False in A/D-only degraded mode
    /// (e.g. Windows PSReadLine under WIN32_INPUT_MODE) — boundary+exit still valid.
    pub has_output_start: bool,
}

/// Surfaced to chat (OSC4 renders these). One per meaningful capability change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandLifecycle {
    /// Output began (`C`). Absent in A/D-only mode.
    Started {
        #[serde(skip_serializing_if = "Option::is_none")]
        command_line: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    /// Command finished (`D`) — always emitted, even A/D-only.
    Finished {
        #[serde(skip_serializing_if = "Option::is_none")]
        command_line: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    /// cwd changed (OSC 7 or 633;P;Cwd), deduped to one source of truth.
    CwdChanged { cwd: String },
}

/// Single source of truth for cwd, deduped across OSC 7 and 633;P;Cwd. Prefers
/// the per-command 633;P;Cwd; OSC 7 is the fallback. No double-count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CwdDetection {
    cwd: Option<String>,
    from_property: bool,
}

impl CwdDetection {
    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    /// 633;P;Cwd — the trusted per-command source. Returns true if cwd changed.
    fn apply_property(&mut self, path: String) -> bool {
        self.from_property = true;
        let changed = self.cwd.as_deref() != Some(path.as_str());
        self.cwd = Some(path);
        changed
    }

    /// OSC 7 — the fallback. Ignored while a per-command Property value stands so
    /// the two sources never double-count. Returns true if cwd changed.
    fn apply_osc7(&mut self, path: String) -> bool {
        if self.from_property {
            return false;
        }
        let changed = self.cwd.as_deref() != Some(path.as_str());
        self.cwd = Some(path);
        changed
    }

    /// New prompt cycle — let the next per-command Property reassert priority.
    fn begin_cycle(&mut self) {
        self.from_property = false;
    }
}

/// Folds command-lifecycle events into a command list. Nonce-checked: a
/// `CommandLine` whose nonce mismatches the session nonce is rejected as
/// untrusted output spoofing.
#[derive(Clone, Debug, Default)]
pub struct CommandDetection {
    nonce: Option<String>,
    commands: Vec<ShellCommand>,
    current: ShellCommand,
    active: bool,
    cwd: CwdDetection,
}

impl CommandDetection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the per-session nonce once known (OSC2 generates it). Idempotent:
    /// only the first non-empty value is taken so a per-chunk re-supply is cheap.
    pub fn ensure_nonce(&mut self, nonce: Option<String>) {
        if self.nonce.is_none() {
            if let Some(n) = nonce.filter(|n| !n.is_empty()) {
                self.nonce = Some(n);
            }
        }
    }

    pub fn commands(&self) -> &[ShellCommand] {
        &self.commands
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.cwd()
    }

    /// Fold one event; returns a lifecycle change to surface, if any.
    pub fn apply(&mut self, ev: SemanticEvent) -> Option<CommandLifecycle> {
        match ev {
            SemanticEvent::PromptStart => {
                self.begin_command();
                None
            }
            // Prompt drawn / input begins — no chat surface on its own.
            SemanticEvent::PromptEnd => None,
            SemanticEvent::CommandStart => {
                self.ensure_active();
                self.current.has_output_start = true;
                Some(CommandLifecycle::Started {
                    command_line: self.current.command_line.clone(),
                    cwd: self.cwd.cwd().map(str::to_string),
                })
            }
            SemanticEvent::CommandLine { text, nonce } => {
                // Nonce guard: reject command text not bearing the session nonce.
                if self.nonce_ok(nonce.as_deref()) {
                    self.ensure_active();
                    self.current.command_line = Some(decode_command(&text));
                }
                None
            }
            SemanticEvent::CommandEnd { exit_code } => self.finish_command(exit_code),
            SemanticEvent::Property { key, value } if key.eq_ignore_ascii_case("Cwd") => {
                let changed = self.cwd.apply_property(value.clone());
                self.current.cwd = Some(value.clone());
                changed.then_some(CommandLifecycle::CwdChanged { cwd: value })
            }
            SemanticEvent::Property { .. } => None,
            SemanticEvent::Cwd { path } => self
                .cwd
                .apply_osc7(path.clone())
                .then_some(CommandLifecycle::CwdChanged { cwd: path }),
        }
    }

    fn nonce_ok(&self, provided: Option<&str>) -> bool {
        match &self.nonce {
            // Configured session nonce: command text MUST carry the exact match.
            Some(expected) => provided == Some(expected.as_str()),
            // No nonce configured yet (OSC2 not wired): can't validate — accept.
            None => true,
        }
    }

    fn begin_command(&mut self) {
        self.current = ShellCommand::default();
        self.active = true;
        self.cwd.begin_cycle();
    }

    // A bare `C`/`E`/`D` can arrive without a preceding `A` (degraded shells);
    // open a command implicitly so its boundary+exit are still captured.
    fn ensure_active(&mut self) {
        if !self.active {
            self.current = ShellCommand::default();
            self.active = true;
        }
    }

    fn finish_command(&mut self, exit_code: Option<i32>) -> Option<CommandLifecycle> {
        self.ensure_active();
        self.current.exit_code = exit_code;
        if self.current.cwd.is_none() {
            self.current.cwd = self.cwd.cwd().map(str::to_string);
        }
        let cmd = std::mem::take(&mut self.current);
        self.active = false;
        let ev = CommandLifecycle::Finished {
            command_line: cmd.command_line.clone(),
            exit_code: cmd.exit_code,
            cwd: cmd.cwd.clone(),
        };
        self.commands.push(cmd);
        Some(ev)
    }
}

/// Decode the VS Code `__VSCode-Escape-Value` scheme: `\\` -> `\`, `\xHH` -> the
/// byte. Inverse of `serializeVSCodeOscMessage` (OSC2 encodes with the same
/// scheme). Non-matching backslash runs pass through literally, as VS Code does.
fn decode_command(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let mut look = chars.clone();
        match look.next() {
            Some('\\') => {
                chars = look;
                out.push('\\');
            }
            Some('x') | Some('X') => {
                let h1 = look.next();
                let h2 = look.next();
                match (
                    h1.and_then(|c| c.to_digit(16)),
                    h2.and_then(|c| c.to_digit(16)),
                ) {
                    (Some(d1), Some(d2)) => {
                        chars = look;
                        out.push(char::from_u32(d1 * 16 + d2).unwrap_or('\u{FFFD}'));
                    }
                    // Not a valid \xHH — leave the backslash literal.
                    _ => out.push('\\'),
                }
            }
            _ => out.push('\\'),
        }
    }
    out
}

/// Per-pane shell-integration state: the OSC1 parser plus the capability fold.
/// `feed` consumes raw PTY bytes and returns the lifecycle changes to surface.
#[derive(Default)]
pub struct ShellIntegration {
    parser: SemanticMarkerParser,
    detection: CommandDetection,
}

impl ShellIntegration {
    pub fn feed(&mut self, data: &[u8]) -> Vec<CommandLifecycle> {
        self.parser
            .feed(data)
            .into_iter()
            .filter_map(|ev| self.detection.apply(ev))
            .collect()
    }

    pub fn detection(&self) -> &CommandDetection {
        &self.detection
    }
}

/// A read-only capability view for consumers (chat / OSC4 query path).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellCapabilitySnapshot {
    pub commands: Vec<ShellCommand>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// Per-pty registry of shell-integration accumulators. Lives beside the
/// `ScreenState`/`PaneStatusTracker` registries; fed from the unified
/// `process_pty_output` path so both the visible and headless readers feed it.
#[derive(Default)]
pub struct ShellCapabilityTracker {
    panes: HashMap<String, ShellIntegration>,
}

impl ShellCapabilityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a raw chunk for `pty_id`; supplies the session nonce (once known) and
    /// returns the lifecycle changes produced by this chunk.
    pub fn feed(
        &mut self,
        pty_id: &str,
        nonce: Option<String>,
        data: &[u8],
    ) -> Vec<CommandLifecycle> {
        let si = self.panes.entry(pty_id.to_string()).or_default();
        si.detection.ensure_nonce(nonce);
        si.feed(data)
    }

    pub fn snapshot(&self, pty_id: &str) -> Option<ShellCapabilitySnapshot> {
        self.panes.get(pty_id).map(|si| ShellCapabilitySnapshot {
            commands: si.detection.commands().to_vec(),
            cwd: si.detection.cwd().map(str::to_string),
        })
    }

    pub fn remove(&mut self, pty_id: &str) {
        self.panes.remove(pty_id);
    }
}

#[cfg(test)]
mod tests;
