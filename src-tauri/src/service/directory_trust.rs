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
) -> bool {
    !trust.owns_prompt() && (session_authenticated || structured_prompt)
}

#[derive(Debug, Default)]
pub(crate) struct TrustLedger {
    tabs: HashMap<String, TabTrust>,
}

#[derive(Debug, Default)]
struct TabTrust {
    answered_frame: Option<u64>,
    in_flight: bool,
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

    fn forget(&mut self, tab_id: &str) {
        self.tabs.remove(tab_id);
    }
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

    pub(crate) fn forget_directory_trust(&self, tab_id: &str) {
        LEDGER.lock().forget(tab_id);
    }
}

#[cfg(test)]
#[path = "directory_trust/tests.rs"]
mod tests;
