use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use codeterm_plugin::provider::DirectoryTrustPromptResponse;

use super::ServiceContext;
use crate::multiplexer::types::Tab;

static LEDGER: LazyLock<parking_lot::Mutex<TrustLedger>> = LazyLock::new(Default::default);
static SHARED_TRUST: LazyLock<parking_lot::Mutex<SharedTrustCache>> =
    LazyLock::new(Default::default);

#[derive(Default)]
struct SharedTrustCache {
    generation: u64,
    trusted: HashMap<String, bool>,
    checking: HashMap<String, u64>,
}

impl SharedTrustCache {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.trusted.clear();
        self.checking.clear();
    }
}
static LAUNCH_ONBOARDING_WORKERS: LazyLock<parking_lot::Mutex<HashMap<String, bool>>> =
    LazyLock::new(Default::default);
static LAUNCH_ONBOARDING_NOTICES: LazyLock<parking_lot::Mutex<HashSet<String>>> =
    LazyLock::new(Default::default);

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

/// Trust prompts are recognized only before this tab's launch session is authenticated.
pub(crate) fn directory_trust_check_applies(tab: &Tab) -> bool {
    tab.agent_type.is_some()
        && !tab.launch_session_authenticated()
        && directory_trust_provenance(tab).is_ok()
}

pub(crate) fn shared_directory_trust_check_applies(tab: &Tab) -> bool {
    tab.agent_type.is_some()
        && !tab.launch_session_authenticated()
        && tab.cwd.as_deref().is_some_and(|cwd| !cwd.is_empty())
        && tab.provisioned_cwd.is_none()
}

pub(crate) fn invalidate_shared_trust_cache() {
    SHARED_TRUST.lock().invalidate();
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
    pub(crate) fn respond_to_pending_directory_trust_prompts(&self) {
        let tabs = {
            let mux = self.mux.read();
            mux.all_tabs()
                .filter(|tab| {
                    tab.pending_prompt_kind == Some(codeterm_types::pty::PromptKind::DirectoryTrust)
                })
                .map(|tab| tab.id.clone())
                .collect::<Vec<_>>()
        };
        for tab_id in tabs {
            self.respond_to_directory_trust(&tab_id, None);
        }
    }

    pub(crate) fn enqueue_launch_onboarding_screen_check(
        &self,
        tab_id: &str,
        pty_id: &str,
        agent_type: &str,
    ) {
        let applies = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pty_id.as_deref() == Some(pty_id)
                && launch_onboarding_check_applies(tab, agent_type)
        });
        if !applies {
            return;
        }
        {
            let mut workers = LAUNCH_ONBOARDING_WORKERS.lock();
            if let Some(pending) = workers.get_mut(tab_id) {
                *pending = true;
                return;
            }
            workers.insert(tab_id.to_string(), false);
        }
        let service = self.clone();
        let tab_id = tab_id.to_string();
        let pty_id = pty_id.to_string();
        let agent_type = agent_type.to_string();
        std::thread::spawn(move || loop {
            service.respond_to_launch_onboarding_until_clear(&tab_id, &pty_id, &agent_type);
            let mut workers = LAUNCH_ONBOARDING_WORKERS.lock();
            if workers.get(&tab_id).copied().unwrap_or(false) {
                workers.insert(tab_id.clone(), false);
            } else {
                workers.remove(&tab_id);
                break;
            }
        });
    }

    fn provider_directory_trust_response(
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

    fn schedule_shared_trust_lookup(&self, tab_id: &str, agent_type: &str, cwd: &str) {
        let key = cwd.to_string();
        let generation = {
            let mut cache = SHARED_TRUST.lock();
            match cache.trusted.get(cwd).copied() {
                Some(true) => None,
                Some(false) => return,
                None => {
                    if cache.checking.contains_key(&key) {
                        return;
                    }
                    let generation = cache.generation;
                    cache.checking.insert(key.clone(), generation);
                    Some(generation)
                }
            }
        };
        let Some(generation) = generation else {
            self.schedule_cached_shared_responder(tab_id, cwd);
            return;
        };
        let service = self.clone();
        let tab = tab_id.to_string();
        let agent = agent_type.to_string();
        let path = cwd.to_string();
        std::thread::spawn(move || {
            let result = service
                .agent_registry
                .with_provider(&agent, |provider| provider.is_directory_trusted(&path));
            let trusted = match result {
                Some(Ok(trusted)) => trusted,
                Some(Err(error)) => {
                    log::warn!("[directory-trust] Trust lookup failed for Tab path: {error}");
                    false
                }
                None => false,
            };
            {
                let mut cache = SHARED_TRUST.lock();
                if cache.generation == generation && cache.checking.get(&path) == Some(&generation)
                {
                    cache.checking.remove(&path);
                    cache.trusted.insert(path.clone(), trusted);
                } else {
                    return;
                }
            }
            if trusted {
                service.respond_to_shared_trust_prompt(&tab, &path);
                service.respond_to_pending_directory_trust_prompts();
            }
        });
    }

    fn schedule_cached_shared_responder(&self, tab_id: &str, cwd: &str) {
        let service = self.clone();
        let tab = tab_id.to_string();
        let path = cwd.to_string();
        std::thread::spawn(move || service.respond_to_shared_trust_prompt(&tab, &path));
    }

    fn respond_to_shared_trust_prompt(&self, tab_id: &str, expected_cwd: &str) {
        let target = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| {
                (shared_directory_trust_check_applies(tab)
                    && tab.cwd.as_deref() == Some(expected_cwd))
                .then(|| Some((tab.pty_id.clone()?, tab.agent_type.clone()?)))
                .flatten()
            })
        };
        let Some((pty_id, agent_type)) = target else {
            return;
        };
        let trusted = SHARED_TRUST
            .lock()
            .trusted
            .get(expected_cwd)
            .copied()
            .unwrap_or(false);
        if !trusted {
            return;
        }
        let screen = self
            .screen_state
            .lock()
            .get_screen_text(&pty_id)
            .unwrap_or_default();
        let Some(response) = self.provider_directory_trust_response(&agent_type, &screen) else {
            return;
        };
        let decision = decide(true, tab_id, &screen, &LEDGER, |_| Some(response));
        if let TrustDecision::Answer { step } = decision {
            self.answer_directory_trust_prompt(
                tab_id,
                &pty_id,
                &agent_type,
                DirectoryTrustPromptResponse::Answer { step },
            );
        }
    }

    pub(crate) fn respond_to_directory_trust(
        &self,
        tab_id: &str,
        screen_text: Option<&str>,
    ) -> TrustDecision {
        let target = {
            let mux = self.mux.read();
            mux.get_tab(tab_id)
                .filter(|tab| {
                    directory_trust_check_applies(tab) || shared_directory_trust_check_applies(tab)
                })
                .and_then(|tab| {
                    Some((
                        tab.pty_id.clone()?,
                        tab.agent_type.clone()?,
                        tab.cwd.clone()?,
                        directory_trust_check_applies(tab),
                    ))
                })
        };
        let Some((pty_id, agent_type, cwd, provisioned)) = target else {
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
        if !provisioned {
            self.schedule_shared_trust_lookup(tab_id, &agent_type, &cwd);
            return if SHARED_TRUST.lock().trusted.get(&cwd) == Some(&true) {
                TrustDecision::Waiting
            } else {
                TrustDecision::NotApplicable
            };
        }
        let decision = decide(true, tab_id, &screen, &LEDGER, |screen| {
            self.provider_directory_trust_response(&agent_type, screen)
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
            self.answer_directory_trust_prompt(
                tab_id,
                &pty_id,
                &agent_type,
                DirectoryTrustPromptResponse::Answer { step: step.clone() },
            );
        }
        decision
    }

    fn answer_directory_trust_prompt(
        &self,
        tab_id: &str,
        pty_id: &str,
        agent_type: &str,
        response: DirectoryTrustPromptResponse,
    ) {
        let DirectoryTrustPromptResponse::Answer { step } = response else {
            return;
        };
        self.prompt_detector.lock().mark_responded(tab_id);
        let svc = self.clone();
        let tab = tab_id.to_string();
        let expected = step.clone();
        let bound_pty = pty_id.to_string();
        let agent = agent_type.to_string();
        super::output::write_bound_prompt_steps(
            self.pty.clone(),
            pty_id.to_string(),
            vec![step.clone()],
            move || svc.directory_trust_key_still_owed(&tab, &bound_pty, &agent, &expected),
        );
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
            .with_provider(&agent_type, |provider| {
                provider.launch_onboarding_response(screen)
            })
            .flatten();
        let Some(response) = response else {
            return TrustDecision::NoDialog;
        };
        let frame_key = format!("launch-onboarding:{}", response.frame_key);
        let expected = response.clone();
        let decision =
            decide_with_frame_identity(true, tab_id, screen, &frame_key, &LEDGER, |_| {
                Some(DirectoryTrustPromptResponse::Answer {
                    step: response.step,
                })
            });
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

    pub(crate) fn respond_to_launch_onboarding_until_clear(
        &self,
        tab_id: &str,
        pty_id: &str,
        agent_type: &str,
    ) -> bool {
        let applies = self.mux.read().get_tab(tab_id).is_some_and(|tab| {
            tab.pty_id.as_deref() == Some(pty_id)
                && launch_onboarding_check_applies(tab, agent_type)
        });
        if !applies {
            return false;
        }
        let signal = self.screen_state.lock().subscribe_screen_changes(pty_id);
        let mut recognized = false;
        let mut enter_submitted = false;
        loop {
            let generation = signal.generation();
            let screen = self
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default();
            let has_dialog = self
                .agent_registry
                .with_provider(agent_type, |provider| {
                    provider.launch_onboarding_response(&screen).is_some()
                })
                .unwrap_or(false);
            if has_dialog {
                recognized = true;
                match self.respond_to_launch_onboarding(tab_id, agent_type, &screen, signal.clone())
                {
                    TrustDecision::Answer { step } => {
                        enter_submitted |= step.as_slice() == b"\r";
                    }
                    TrustDecision::Waiting => {}
                    TrustDecision::NotApplicable => {
                        recognized = false;
                        break;
                    }
                    TrustDecision::NoDialog | TrustDecision::Held => {
                        break;
                    }
                }
            } else if recognized {
                if enter_submitted {
                    self.notify_launch_onboarding_applied_once(tab_id);
                }
                break;
            } else {
                break;
            }
            if !signal.wait_until_changed(generation) {
                break;
            }
        }
        self.screen_state
            .lock()
            .unsubscribe_screen_changes(pty_id, &signal);
        recognized
    }

    pub(crate) fn notify_launch_onboarding_applied_once(&self, tab_id: &str) {
        if !LAUNCH_ONBOARDING_NOTICES.lock().insert(tab_id.to_string()) {
            return;
        }
        self.create_notification_ext(
            "Provider onboarding choice applied".into(),
            Some(format!(
                "Tab {} completed its provider-declared safe onboarding choice.",
                tab_id
            )),
            Some("info"),
            None,
            None,
            crate::service::notification::NotificationOrigin::local_tab(tab_id.to_string()),
            crate::service::notification::PushIntent::default(),
            None,
        );
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
            log::info!("[launch-onboarding] Withheld stale key for Tab {}", tab_id);
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
        let cwd = self.mux.read().get_tab(tab_id).and_then(|tab| {
            (tab.pty_id.as_deref() == Some(pty_id)
                && (directory_trust_check_applies(tab)
                    || shared_directory_trust_check_applies(tab)))
            .then(|| tab.cwd.clone())
            .flatten()
        });
        let (owed, dialog_still_answerable) = cwd.map_or((false, false), |cwd| {
            let screen = self
                .screen_state
                .lock()
                .get_screen_text(pty_id)
                .unwrap_or_default();
            let provisioned = self
                .mux
                .read()
                .get_tab(tab_id)
                .is_some_and(directory_trust_check_applies);
            let eligible = provisioned || SHARED_TRUST.lock().trusted.get(&cwd) == Some(&true);
            match self.provider_directory_trust_response(agent_type, &screen) {
                Some(DirectoryTrustPromptResponse::Answer { step }) if eligible => {
                    (step == expected, true)
                }
                _ => (false, false),
            }
        });
        LEDGER.lock().settle(tab_id);
        if !owed {
            log::info!(
                "[directory-trust] Withheld key for Tab {}: the dialog changed before the write",
                tab_id
            );
            // A settled dialog may emit no further frame, so re-evaluate once against the current one.
            if dialog_still_answerable {
                let service = self.clone();
                let tab = tab_id.to_string();
                std::thread::spawn(move || {
                    service.respond_to_directory_trust(&tab, None);
                });
            }
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
            "[directory-trust] Not answering Tab {}: {}; screen tail: {}",
            tab_id,
            outcome,
            tail.as_deref().unwrap_or("-")
        );
    }

    pub(crate) fn forget_directory_trust(&self, tab_id: &str) {
        LEDGER.lock().forget(tab_id);
    }
}

pub(crate) fn launch_onboarding_check_applies(tab: &Tab, agent_type: &str) -> bool {
    tab.agent_type.as_deref() == Some(agent_type)
        && (tab.role == Some(codeterm_types::agent::roles::AgentRole::Worker)
            || (tab.role == Some(codeterm_types::agent::roles::AgentRole::Manager)
                && tab.role_profile.as_deref() == Some("planner")))
}

#[cfg(test)]
#[path = "directory_trust/tests.rs"]
mod tests;
