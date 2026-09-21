use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use codeterm_plugin::provider::DirectoryTrustPromptResponse;

use super::ServiceContext;
use crate::multiplexer::types::Tab;

static LEDGER: LazyLock<parking_lot::Mutex<TrustLedger>> = LazyLock::new(Default::default);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectoryTrustRefusal {
    NoProvenance,
    NoCwd,
    CwdDiffersFromProvenance,
}

impl DirectoryTrustRefusal {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::NoProvenance => "cwd was not created by CodeTerm for this tab",
            Self::NoCwd => "tab has no cwd",
            Self::CwdDiffersFromProvenance => "cwd differs from the directory CodeTerm created",
        }
    }
}

const NOTE_BUDGET: usize = 16;

pub(crate) fn directory_trust_prompt_candidate(screen: &str) -> bool {
    let rows = screen.lines().collect::<Vec<_>>();
    let options = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| numbered_menu_option(row).map(|option| (index, option)))
        .collect::<Vec<_>>();
    let Some(last_option) = options.last().cloned() else {
        return false;
    };
    let mut active_options = vec![last_option];
    for option in options.iter().rev().skip(1) {
        if active_options[0].0.abs_diff(option.0) > 2 {
            break;
        }
        active_options.insert(0, option.clone());
    }
    if active_options.len() != 2 {
        return false;
    }
    let first_choice = active_options[0].0;
    let last_choice = active_options[1].0;
    let selected = active_options
        .iter()
        .filter(|(_, option)| option.highlighted)
        .count();
    if selected != 1
        || active_options[0].1.number != 1
        || active_options[1].1.number != 2
        || !active_options[0]
            .1
            .label
            .to_ascii_lowercase()
            .starts_with("yes")
        || !active_options[1]
            .1
            .label
            .to_ascii_lowercase()
            .starts_with("no")
    {
        return false;
    }
    let footer = (last_choice + 1..rows.len().min(last_choice + 5)).find(|index| {
        let row = rows[*index].to_ascii_lowercase();
        row.contains("enter") && (row.contains("continue") || row.contains("select"))
    });
    let Some(footer) = footer else {
        return false;
    };
    let last_meaningful = rows.iter().rposition(|row| {
        let row = row.trim();
        !row.is_empty() && !row.chars().all(|ch| matches!(ch, '─' | '━' | '═' | '-'))
    });
    if last_meaningful != Some(footer) {
        return false;
    }
    let context_rows = &rows[first_choice.saturating_sub(12)..first_choice];
    if !context_rows
        .iter()
        .any(|row| directory_context_row(row))
    {
        return false;
    }
    let mut preamble_start = first_choice;
    while preamble_start > 0 && rows[preamble_start - 1].trim().is_empty() {
        preamble_start -= 1;
    }
    while preamble_start > 0 && !rows[preamble_start - 1].trim().is_empty() {
        preamble_start -= 1;
    }
    let preamble = rows[preamble_start..first_choice]
        .iter()
        .filter(|row| !directory_context_row(row))
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    directory_trust_content(&preamble)
}

#[derive(Clone)]
struct NumberedMenuOption {
    number: usize,
    label: String,
    highlighted: bool,
}

fn numbered_menu_option(row: &str) -> Option<NumberedMenuOption> {
    let row = row.trim_start();
    let first = row.chars().next()?;
    let (row, highlighted) = match first {
        '>' | '❯' | '▶' | '➤' | '›' => (&row[first.len_utf8()..], true),
        _ => (row, false),
    };
    let row = row.trim_start();
    let digit_count = row.chars().take_while(char::is_ascii_digit).count();
    let number = row.get(..digit_count)?.parse::<usize>().ok()?;
    let suffix = row.get(digit_count..)?.trim_start();
    let separator = suffix.chars().next()?;
    if !matches!(separator, '.' | ')') {
        return None;
    }
    let label = suffix.get(separator.len_utf8()..)?.trim_start();
    (!label.is_empty()).then(|| NumberedMenuOption {
        number,
        label: label.to_string(),
        highlighted,
    })
}

fn directory_context_row(row: &str) -> bool {
    let row = row.trim();
    let context = row
        .strip_prefix('>')
        .or_else(|| row.strip_prefix('❯'))
        .or_else(|| row.strip_prefix('›'));
    let Some(context) = context else {
        return false;
    };
    context
        .split_whitespace()
        .any(|path| absolute_path_token(path))
}

fn absolute_path_token(token: &str) -> bool {
    let path = token.trim_end_matches(|ch| matches!(ch, ')' | ',' | ';'));
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with(r"\\")
        || path.starts_with("//")
        || (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
}

fn directory_trust_content(preamble: &str) -> bool {
    let content = preamble.to_ascii_lowercase();
    let words = content
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if ["bypass", "disable", "override"]
        .iter()
        .any(|term| words.contains(term))
        || content.contains("without restrictions")
    {
        return false;
    }
    let Some((question, explanation)) = content.split_once('?') else {
        return false;
    };
    let has_directory = ["folder", "directory", "workspace"].iter().any(|term| {
        question
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .any(|word| word == *term)
    });
    let has_trust_intent = ["trust", "trusted", "trusting", "load", "loaded", "loading"]
        .iter()
        .any(|term| {
            question
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|word| word == *term)
        });
    let has_local_policy = [
        "guidance",
        "rules",
        "config",
        "configuration",
        "settings",
        "policy",
        "policies",
        "instructions",
        "hooks",
    ]
    .iter()
    .any(|term| {
        explanation
            .split(|ch: char| !ch.is_ascii_alphanumeric())
            .any(|word| word == *term)
    });
    let explanation_words = explanation
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .count();
    has_directory && has_trust_intent && has_local_policy && explanation_words >= 8
}

/// Provenance is host state written by the code that created the directory; a
/// spawn-supplied cwd never carries it, so it can never self-authorize trust.
pub(crate) fn directory_trust_provenance(tab: &Tab) -> Result<(), DirectoryTrustRefusal> {
    let provisioned = tab
        .provisioned_cwd
        .as_deref()
        .ok_or(DirectoryTrustRefusal::NoProvenance)?;
    let cwd = tab.cwd.as_deref().ok_or(DirectoryTrustRefusal::NoCwd)?;
    if crate::service::ServiceContext::paths_equal(cwd, provisioned) {
        Ok(())
    } else {
        Err(DirectoryTrustRefusal::CwdDiffersFromProvenance)
    }
}

/// A spawn inherits workspace provenance only when it runs in exactly the daemon-created directory.
pub(crate) fn inherited_workspace_provenance(
    resolved_cwd: Option<&str>,
    workspace_provisioned_cwd: Option<&str>,
) -> Option<String> {
    let provisioned = workspace_provisioned_cwd?;
    let cwd = resolved_cwd?;
    crate::service::ServiceContext::paths_equal(cwd, provisioned).then(|| provisioned.to_string())
}

/// The recognizer runs only for an agent in the directory CodeTerm created whose
/// launch session is not yet authenticated; the trust dialog precedes that binding.
pub(crate) fn directory_trust_check_applies(tab: &Tab) -> bool {
    tab.agent_type.is_some()
        && !tab.launch_session_authenticated()
        && directory_trust_provenance(tab).is_ok()
}

/// Why a provisioned agent tab skips the trust responder; `None` when it
/// applies or the tab carries no provenance at all.
pub(crate) fn directory_trust_skip_reason(tab: &Tab) -> Option<&'static str> {
    tab.provisioned_cwd.as_ref()?;
    tab.agent_type.as_ref()?;
    if tab.launch_session_authenticated() {
        return Some("launch session already authenticated");
    }
    directory_trust_provenance(tab)
        .err()
        .map(DirectoryTrustRefusal::reason)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TrustDecision {
    NotApplicable,
    NoDialog,
    Held,
    /// A key is in flight, or this exact frame was already answered.
    Waiting,
    Answer {
        step: Vec<u8>,
    },
}

impl TrustDecision {
    pub(crate) fn owns_prompt(&self) -> bool {
        matches!(self, Self::Held | Self::Waiting | Self::Answer { .. })
    }
}

/// The blind yes-responder never sees a screen-scraped prompt before the launch
/// session is authenticated, which is where every trust dialog lives.
pub(crate) fn legacy_auto_approve_permitted(
    trust: &TrustDecision,
    session_authenticated: bool,
    structured_prompt: bool,
    directory_trust_prompt: bool,
) -> bool {
    !directory_trust_prompt
        && !trust.owns_prompt()
        && (session_authenticated || structured_prompt)
}

#[derive(Debug, Default)]
pub(crate) struct TrustLedger {
    tabs: HashMap<String, TabTrust>,
}

#[derive(Debug, Default)]
struct TabTrust {
    answered_frame: Option<u64>,
    in_flight: bool,
    last_note: Option<(&'static str, u64)>,
    notes: usize,
}

impl TrustLedger {
    fn claim(&mut self, tab_id: &str, frame: u64) -> bool {
        let tab = self.tabs.entry(tab_id.to_string()).or_default();
        if tab.in_flight || tab.answered_frame == Some(frame) {
            return false;
        }
        tab.in_flight = true;
        tab.answered_frame = Some(frame);
        true
    }

    pub(crate) fn settle(&mut self, tab_id: &str) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.in_flight = false;
        }
    }

    /// True when this outcome differs from the last one noted for the tab and
    /// the tab still has log budget; a static screen is noted once.
    pub(crate) fn note(&mut self, tab_id: &str, outcome: &'static str, detail: u64) -> bool {
        let tab = self.tabs.entry(tab_id.to_string()).or_default();
        if tab.last_note == Some((outcome, detail)) || tab.notes >= NOTE_BUDGET {
            return false;
        }
        tab.last_note = Some((outcome, detail));
        tab.notes += 1;
        true
    }

    fn forget(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
    }
}

fn screen_tail(screen: &str) -> String {
    let rows: Vec<&str> = screen
        .lines()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .collect();
    rows[rows.len().saturating_sub(4)..]
        .join(" | ")
        .chars()
        .take(240)
        .collect()
}

fn frame_id(screen: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    screen.hash(&mut hasher);
    hasher.finish()
}

/// One key per observed frame: a new key is claimed only once the previous key
/// has been written and the screen has changed since it was decided.
pub(crate) fn decide<R>(
    applies: bool,
    tab_id: &str,
    screen: &str,
    ledger: &parking_lot::Mutex<TrustLedger>,
    recognize: R,
) -> TrustDecision
where
    R: FnOnce(&str) -> Option<DirectoryTrustPromptResponse>,
{
    if !applies {
        return TrustDecision::NotApplicable;
    }
    match recognize(screen) {
        None => TrustDecision::NoDialog,
        Some(DirectoryTrustPromptResponse::Hold) => TrustDecision::Held,
        Some(DirectoryTrustPromptResponse::Answer { step }) => {
            if ledger.lock().claim(tab_id, frame_id(screen)) {
                TrustDecision::Answer { step }
            } else {
                TrustDecision::Waiting
            }
        }
    }
}

fn key_name(step: &[u8]) -> &'static str {
    match step {
        b"\r" => "enter",
        b"j" | b"k" => "a move key",
        _ => "a provider key",
    }
}

impl ServiceContext {
    fn directory_trust_response(
        &self,
        agent_type: &str,
        screen: &str,
    ) -> Option<DirectoryTrustPromptResponse> {
        self.agent_registry
            .with_provider(agent_type, |provider| {
                provider.directory_trust_prompt_response(screen)
            })
            .flatten()
    }

    pub(crate) fn respond_to_directory_trust(
        &self,
        tab_id: &str,
        screen_text: Option<&str>,
    ) -> TrustDecision {
        let target = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| directory_trust_check_applies(tab))
                .and_then(|tab| Some((tab.pty_id.clone()?, tab.agent_type.clone()?)))
        };
        let Some((pty_id, agent_type)) = target else {
            return TrustDecision::NotApplicable;
        };
        let screen = match screen_text {
            Some(text) => text.to_string(),
            None => self
                .screen_state
                .lock()
                .get_screen_text(&pty_id)
                .unwrap_or_default(),
        };
        let decision = decide(true, tab_id, &screen, &LEDGER, |screen| {
            self.directory_trust_response(&agent_type, screen)
        });
        match &decision {
            TrustDecision::NoDialog
                if crate::agent::detector::detect_interactive_prompt(&screen) =>
            {
                self.note_directory_trust(tab_id, "no trust dialog recognized", Some(&screen));
            }
            TrustDecision::Held => {
                self.note_directory_trust(
                    tab_id,
                    "provider holds this trust dialog",
                    Some(&screen),
                );
            }
            _ => {}
        }
        if let TrustDecision::Answer { step } = &decision {
            log::info!(
                "[directory-trust] Answering directory trust dialog for Tab {} with {}",
                tab_id,
                key_name(step)
            );
            self.prompt_detector.lock().mark_responded(tab_id);
            let svc = self.clone();
            let tab = tab_id.to_string();
            let expected = step.clone();
            let bound_pty = pty_id.clone();
            super::output::write_bound_prompt_steps(
                self.pty.clone(),
                pty_id,
                vec![step.clone()],
                move || {
                    svc.directory_trust_key_still_owed(&tab, &bound_pty, &agent_type, &expected)
                },
            );
        }
        decision
    }

    fn directory_trust_key_still_owed(
        &self,
        tab_id: &str,
        pty_id: &str,
        agent_type: &str,
        expected: &[u8],
    ) -> bool {
        let applies = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pty_id.as_deref() == Some(pty_id) && directory_trust_check_applies(tab)
        });
        let owed = applies && {
            let screen = self
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default();
            matches!(
                self.directory_trust_response(agent_type, &screen),
                Some(DirectoryTrustPromptResponse::Answer { step }) if step == expected
            )
        };
        LEDGER.lock().settle(tab_id);
        if !owed {
            log::info!(
                "[directory-trust] Withheld key for Tab {}: the dialog changed before the write",
                tab_id
            );
        }
        owed
    }

    pub(crate) fn note_directory_trust(
        &self,
        tab_id: &str,
        outcome: &'static str,
        screen: Option<&str>,
    ) {
        let tail = screen.map(screen_tail);
        let detail = tail.as_deref().map(frame_id).unwrap_or(0);
        if !LEDGER.lock().note(tab_id, outcome, detail) {
            return;
        }
        log::info!(
            "[directory-trust] Not answering provisioned Tab {}: {}; screen tail: {}",
            tab_id,
            outcome,
            tail.as_deref().unwrap_or("-")
        );
    }

    pub(crate) fn forget_directory_trust(&self, tab_id: &str) {
        LEDGER.lock().forget(tab_id);
    }
}

#[cfg(test)]
#[path = "directory_trust/tests.rs"]
mod tests;
