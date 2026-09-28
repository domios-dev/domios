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
    !directory_trust_prompt && !trust.owns_prompt() && (session_authenticated || structured_prompt)
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
    decide_with_frame_identity(applies, tab_id, screen, screen, ledger, recognize)
}

fn decide_with_frame_identity<R>(
    applies: bool,
    tab_id: &str,
    screen: &str,
    frame_identity: &str,
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
            if ledger.lock().claim(tab_id, frame_id(frame_identity)) {
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

    pub(crate) fn respond_to_launch_onboarding(
        &self,
        tab_id: &str,
        agent_type: &str,
        screen: &str,
        response_signal: std::sync::Arc<codeterm_pty::pty::screen_state::ScreenChangeSignal>,
    ) -> TrustDecision {
        let target = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| launch_onboarding_check_applies(tab, agent_type))
                .and_then(|tab| Some((tab.pty_id.clone()?, tab.agent_type.clone()?)))
        };
        let Some((pty_id, agent_type)) = target else {
            return TrustDecision::NotApplicable;
        };
        let response = self
            .agent_registry
            .with_provider(&agent_type, |provider| provider.launch_onboarding_response(screen))
            .flatten();
        let Some(response) = response else {
            return TrustDecision::NoDialog;
        };
        let frame_key = format!("launch-onboarding:{}", response.frame_key);
        let expected = response.clone();
        let decision = decide_with_frame_identity(
            true,
            tab_id,
            screen,
            &frame_key,
            &LEDGER,
            |_| {
                Some(DirectoryTrustPromptResponse::Answer {
                    step: response.step,
                })
            },
        );
        if let TrustDecision::Answer { step } = &decision {
            self.prompt_detector.lock().mark_responded(tab_id);
            let service = self.clone();
            let tab = tab_id.to_string();
            let bound_pty = pty_id.clone();
            let provider_id = agent_type.clone();
            let response_signal = response_signal.clone();
            super::output::write_bound_prompt_steps(
                self.pty.clone(),
                pty_id,
                vec![step.clone()],
                move || {
                    let owed = service.launch_onboarding_key_still_owed(
                        &tab,
                        &bound_pty,
                        &provider_id,
                        &expected,
                    );
                    response_signal.mark_changed();
                    owed
                },
            );
        }
        decision
    }

    fn launch_onboarding_key_still_owed(
        &self,
        tab_id: &str,
        pty_id: &str,
        agent_type: &str,
        expected: &codeterm_plugin::provider::ProviderPromptKeyStep,
    ) -> bool {
        let applies = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pty_id.as_deref() == Some(pty_id)
                && launch_onboarding_check_applies(tab, agent_type)
        });
        let owed = applies && {
            let screen = self
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default();
            self.agent_registry
                .with_provider(agent_type, |provider| {
                    provider
                        .launch_onboarding_response(&screen)
                        .is_some_and(|response| response == *expected)
                })
                .unwrap_or(false)
        };
        LEDGER.lock().settle(tab_id);
        if !owed {
            log::info!(
                "[launch-onboarding] Withheld stale key for Tab {}",
                tab_id
            );
        }
        owed
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

fn launch_onboarding_check_applies(tab: &Tab, agent_type: &str) -> bool {
    tab.agent_type.as_deref() == Some(agent_type)
        && !tab.launch_session_authenticated()
        && (tab.role == Some(codeterm_types::agent::roles::AgentRole::Worker)
            || (tab.role == Some(codeterm_types::agent::roles::AgentRole::Manager)
                && tab.role_profile.as_deref() == Some("planner")))
}

#[cfg(test)]
#[path = "directory_trust/tests.rs"]
mod tests;
