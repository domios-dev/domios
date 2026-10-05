use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock};

use codeterm_plugin::tab_state::TabState;
use codeterm_types::pty::{PendingPrompt, PromptKind};
use parking_lot::Mutex;

const MAX_AUTOMATIC_ANSWERS: usize = 5;
const MAX_TRACKED_LAUNCHES: usize = 256;

static CHAINS: LazyLock<Mutex<PromptChainBook>> = LazyLock::new(Default::default);
static WATCHERS: LazyLock<
    Mutex<HashMap<ChainKey, Vec<Arc<codeterm_pty::pty::screen_state::ScreenChangeSignal>>>>,
> = LazyLock::new(Default::default);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ChainKey {
    tab_id: String,
    launch_id: String,
}

#[derive(Default)]
struct ChainProgress {
    answers: usize,
    active: bool,
    last_prompt: Option<String>,
    notices: HashSet<String>,
}

#[derive(Default)]
struct PromptChainBook {
    entries: HashMap<ChainKey, ChainProgress>,
    insertion_order: VecDeque<ChainKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    Allowed,
    Repeated,
    Limit,
}

impl PromptChainBook {
    fn progress(&mut self, key: &ChainKey) -> &mut ChainProgress {
        if !self.entries.contains_key(key) {
            self.insertion_order.push_back(key.clone());
            self.entries.insert(key.clone(), ChainProgress::default());
            while self.entries.len() > MAX_TRACKED_LAUNCHES {
                if let Some(expired) = self.insertion_order.pop_front() {
                    self.entries.remove(&expired);
                }
            }
        }
        self.entries.get_mut(key).expect("inserted prompt chain")
    }

    fn admit(&mut self, key: &ChainKey, fingerprint: String) -> Admission {
        let progress = self.progress(key);
        if progress.answers >= MAX_AUTOMATIC_ANSWERS {
            progress.active = false;
            return Admission::Limit;
        }
        if progress.last_prompt.as_deref() == Some(fingerprint.as_str()) {
            progress.active = false;
            return Admission::Repeated;
        }
        progress.answers += 1;
        progress.last_prompt = Some(fingerprint);
        progress.active = true;
        Admission::Allowed
    }

    fn is_active(&self, key: &ChainKey) -> bool {
        self.entries.get(key).is_some_and(|progress| progress.active)
    }

    fn blocks_keys(&self, key: &ChainKey) -> bool {
        self.entries
            .get(key)
            .is_some_and(|progress| progress.active || !progress.notices.is_empty())
    }

    fn finish(&mut self, key: &ChainKey) {
        if let Some(progress) = self.entries.get_mut(key) {
            progress.active = false;
        }
    }

    fn claim_notice(&mut self, key: &ChainKey, kind: &str) -> bool {
        self.progress(key).notices.insert(kind.to_string())
    }
}

fn fingerprint(kind: PromptKind, prompt: &PendingPrompt) -> String {
    let encoded = serde_json::to_vec(&serde_json::json!({
        "kind": kind,
        "question": prompt.question,
        "options": prompt.options,
        "sections": prompt.sections,
        "multi_select": prompt.multi_select,
    }))
    .unwrap_or_default();
    hash_bytes(&encoded)
}

fn blocker_kind(prompt: &PendingPrompt) -> &'static str {
    if prompt.multi_select == Some(true) {
        "multi_select"
    } else if prompt.options.is_empty() {
        "free_text"
    } else if prompt.options.len() > 10 {
        "too_many_options"
    } else {
        "unsupported_prompt"
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn key_for_tab(tab: &crate::multiplexer::types::Tab) -> ChainKey {
    let launch_id = match tab.session_binding_nonce.as_deref() {
        Some(nonce) => format!("{}:pty:{:?}:nonce:{nonce}", tab.id, tab.pty_id),
        None => crate::service::supporter_tab_state::launch_episode_key(
            &tab.id,
            tab.pty_id.as_deref(),
            tab.agent_set_at,
        ),
    };
    ChainKey {
        tab_id: tab.id.clone(),
        launch_id,
    }
}

fn notice_body(subject: &str, kind: &str) -> String {
    match kind {
        "login" => format!("{subject} needs login or first-run setup."),
        "quota" => format!(
            "{subject} needs a model availability, quota, or rate-limit issue resolved."
        ),
        "unavailable_model" => format!("{subject} cannot use its requested model."),
        "free_text" => format!("{subject} needs free-text input, such as an API key."),
        "multi_select" => format!("{subject} needs its multi-select prompt answered."),
        "too_many_options" => {
            format!("{subject} needs an owner choice from more than ten options.")
        }
        "unsupported_prompt" => format!("{subject} needs its unsupported prompt answered."),
        "unknown_dialog" => format!("{subject} is blocked on an unrecognized dialog."),
        "answer_limit" => format!("{subject} reached the automatic prompt answer limit."),
        kind if kind.starts_with("repeated:") => {
            format!("{subject} repeated a prompt after an automatic answer.")
        }
        _ => format!("{subject} needs attention before it can continue."),
    }
}

impl super::ServiceContext {
    fn prompt_chain_key(&self, tab_id: &str) -> Option<ChainKey> {
        let mux = self.mux.read();
        let tab = mux.get_tab(tab_id)?;
        crate::service::unattended_launch::owns_prompt_answers(tab).then(|| key_for_tab(tab))
    }

    pub(crate) fn prompt_chain_blocks_keys(&self, tab_id: &str) -> bool {
        self.prompt_chain_key(tab_id)
            .is_some_and(|key| CHAINS.lock().blocks_keys(&key))
    }

    pub(crate) fn begin_automatic_prompt_answer(
        &self,
        tab_id: &str,
        kind: &str,
        prompt_fingerprint: String,
    ) -> bool {
        let Some(key) = self.prompt_chain_key(tab_id) else {
            return true;
        };
        let target = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| {
                let pty_id = tab.pty_id.clone()?;
                let agent_type = tab.agent_type.clone()?;
                let prior_screen = self
                    .screen_state
                    .lock()
                    .get_screen_text(&pty_id)
                    .unwrap_or_default();
                Some((pty_id, agent_type, prior_screen))
            })
        };
        let Some((pty_id, agent_type, prior_screen)) = target else {
            self.finish_prompt_chain(&key);
            self.notify_prompt_chain_once(&key, "unknown_dialog", "unknown_dialog");
            return false;
        };
        let prompt_fingerprint = self
            .agent_registry
            .provider_arc(&agent_type)
            .and_then(|provider| {
                provider
                    .parse_directory_trust_prompt(&prior_screen)
                    .map(|prompt| (prompt, PromptKind::DirectoryTrust))
                    .or_else(|| {
                        provider
                            .parse_prompt(&prior_screen)
                            .map(|prompt| (prompt, PromptKind::Provider))
                    })
            })
            .map(|(prompt, kind)| fingerprint(kind, &prompt))
            .unwrap_or(prompt_fingerprint);
        let admission = CHAINS.lock().admit(&key, prompt_fingerprint);
        match admission {
            Admission::Allowed => {
                self.watch_prompt_chain(key, pty_id, agent_type, prior_screen);
                true
            }
            Admission::Repeated => {
                self.finish_prompt_chain(&key);
                self.notify_prompt_chain_once(
                    &key,
                    &format!("repeated:{kind}"),
                    "repeated:prompt",
                );
                false
            }
            Admission::Limit => {
                self.finish_prompt_chain(&key);
                self.notify_prompt_chain_once(&key, "answer_limit", "answer_limit");
                false
            }
        }
    }

    pub(crate) fn prompt_chain_fingerprint(
        &self,
        kind: PromptKind,
        prompt: &PendingPrompt,
    ) -> String {
        fingerprint(kind, prompt)
    }

    pub(crate) fn handle_prompt_chain_verdict(
        &self,
        tab_id: &str,
        label: &str,
        has_unparsed_modal: bool,
    ) -> bool {
        let Some(key) = self.prompt_chain_key(tab_id) else {
            return false;
        };
        let notice = match label {
            crate::service::supporter_tab_state::LOGIN_OR_AUTH_REQUIRED => Some("login"),
            crate::service::supporter_tab_state::MODEL_UNAVAILABLE_OR_QUOTA => Some("quota"),
            crate::service::supporter_tab_state::OTHER
                if has_unparsed_modal && CHAINS.lock().is_active(&key) =>
            {
                Some("unknown_dialog")
            }
            crate::service::supporter_tab_state::READY_IDLE
            | crate::service::supporter_tab_state::WORKING_NORMALLY
                if CHAINS.lock().is_active(&key) =>
            {
                self.finish_prompt_chain(&key);
                return false;
            }
            _ => None,
        };
        if let Some(kind) = notice {
            self.finish_prompt_chain(&key);
            self.notify_prompt_chain_once(&key, kind, kind);
            return true;
        }
        false
    }

    pub(crate) fn handle_prompt_chain_blocker(
        &self,
        tab_id: &str,
        prompt: &PendingPrompt,
        kind: PromptKind,
    ) -> bool {
        let Some(key) = self.prompt_chain_key(tab_id) else {
            return false;
        };
        if !CHAINS.lock().is_active(&key) {
            return false;
        }
        self.finish_prompt_chain(&key);
        self.notify_prompt_chain_once(
            &key,
            &format!("prompt:{kind:?}"),
            blocker_kind(prompt),
        );
        true
    }

    fn finish_prompt_chain(&self, key: &ChainKey) {
        CHAINS.lock().finish(key);
        if let Some(signals) = WATCHERS.lock().get(key) {
            for signal in signals {
                signal.mark_changed();
            }
        }
    }

    fn notify_prompt_chain_once(&self, key: &ChainKey, dedupe_kind: &str, message_kind: &str) {
        if !CHAINS.lock().claim_notice(key, dedupe_kind) {
            return;
        }
        let subject = {
            let mux = self.mux.read();
            mux.get_tab(&key.tab_id)
                .map(|_| mux.display_label_for_tab(&key.tab_id))
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| key.tab_id.clone())
        };
        let body = notice_body(&subject, message_kind);
        let event_hash = hash_bytes(
            serde_json::to_string(&(&key.launch_id, dedupe_kind))
                .unwrap_or_default()
                .as_bytes(),
        );
        self.create_notification_ext(
            "Agent launch needs your attention".into(),
            Some(body),
            Some("warning"),
            None,
            Some(true),
            crate::service::notification::NotificationOrigin::local_tab(key.tab_id.clone()),
            crate::service::notification::PushIntent {
                level: Some(crate::notification::push::NotifyLevel::ActionNeeded),
                event_key: Some(format!("prompt-chain:{event_hash}")),
            },
            None,
        );
    }

    fn watch_prompt_chain(
        &self,
        key: ChainKey,
        pty_id: String,
        agent_type: String,
        prior_screen: String,
    ) {
        let signal = self.screen_state.lock().subscribe_screen_changes(&pty_id);
        let mut generation = signal.generation();
        WATCHERS
            .lock()
            .entry(key.clone())
            .or_default()
            .push(signal.clone());
        let service = self.clone();
        std::thread::spawn(move || {
            loop {
                if !service.prompt_chain_is_current(&key) || !signal.wait_until_changed(generation) {
                    break;
                }
                generation = signal.generation();
                let screen = service
                    .screen_state
                    .lock()
                    .get_screen_text(&pty_id)
                    .unwrap_or_default();
                if screen == prior_screen {
                    continue;
                }
                let identity = {
                    let mux = service.mux.read();
                    mux.get_tab(&key.tab_id).and_then(|tab| {
                        (tab.pty_id.as_deref() == Some(pty_id.as_str()))
                            .then(|| (tab.agent_session_id.clone(), tab.cwd.clone()))
                    })
                };
                let Some((session_id, cwd)) = identity else {
                    break;
                };
                let snapshot = crate::agent::tab_state::TabStateSnapshot {
                    tab_id: key.tab_id.clone(),
                    agent_type: agent_type.clone(),
                    session_id,
                    cwd,
                    screen_text: screen.clone(),
                };
                let Some(provider) = service.agent_registry.provider_arc(&agent_type) else {
                    service.finish_prompt_chain(&key);
                    service.notify_prompt_chain_once(&key, "unknown_dialog", "unknown_dialog");
                    break;
                };
                let classification = provider.classify_tab_state(&snapshot);
                let prompt = provider
                    .parse_directory_trust_prompt(&screen)
                    .map(|prompt| (prompt, PromptKind::DirectoryTrust))
                    .or_else(|| {
                        provider
                            .parse_prompt(&screen)
                            .map(|prompt| (prompt, PromptKind::Provider))
                    })
                    .filter(|(prompt, kind)| {
                        *kind == PromptKind::DirectoryTrust
                            || (crate::agent::state_reactor::has_structural_prompt_evidence(
                                &screen,
                                prompt,
                            ) && !crate::agent::state_reactor::prompt_is_composer_text(
                                &screen, prompt,
                            ))
                    });
                if let Some((prompt, kind)) = prompt.as_ref() {
                    let unanswerable_shape = prompt.multi_select == Some(true)
                        || prompt.options.is_empty()
                        || prompt.options.len() > 10;
                    if unanswerable_shape
                        && service.handle_prompt_chain_blocker(&key.tab_id, prompt, *kind)
                    {
                        break;
                    }
                }
                if let Some(classification) = classification {
                    match classification.state {
                        TabState::WaitingOnAuth
                            if classification.confidence
                                >= crate::agent::state_reactor::GLOBAL_THRESHOLD =>
                        {
                            service.finish_prompt_chain(&key);
                            service.notify_prompt_chain_once(&key, "login", "login");
                            break;
                        }
                        TabState::RateLimited
                            if classification.confidence
                                >= crate::agent::state_reactor::GLOBAL_THRESHOLD =>
                        {
                            service.finish_prompt_chain(&key);
                            service.notify_prompt_chain_once(&key, "quota", "quota");
                            break;
                        }
                        TabState::ClarifyingQuestion
                            if prompt.is_none()
                                && classification.confidence
                                    >= crate::agent::state_reactor::GLOBAL_THRESHOLD =>
                        {
                            service.finish_prompt_chain(&key);
                            service.notify_prompt_chain_once(&key, "free_text", "free_text");
                            break;
                        }
                        TabState::Unknown
                            if prompt.is_none()
                                && crate::agent::detector::detect_interactive_prompt(&screen) =>
                        {
                            service.finish_prompt_chain(&key);
                            service.notify_prompt_chain_once(
                                &key,
                                "unknown_dialog",
                                "unknown_dialog",
                            );
                            break;
                        }
                        _ => {}
                    }
                }
                if prompt.is_some() {
                    break;
                }
            }
            service
                .screen_state
                .lock()
                .unsubscribe_screen_changes(&pty_id, &signal);
            let mut watchers = WATCHERS.lock();
            if let Some(signals) = watchers.get_mut(&key) {
                signals.retain(|watched| !Arc::ptr_eq(watched, &signal));
                if signals.is_empty() {
                    watchers.remove(&key);
                }
            }
        });
    }

    fn prompt_chain_is_current(&self, key: &ChainKey) -> bool {
        self.prompt_chain_key(&key.tab_id)
            .is_some_and(|current| current == *key && CHAINS.lock().is_active(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str) -> ChainKey {
        ChainKey {
            tab_id: id.into(),
            launch_id: format!("launch-{id}"),
        }
    }

    #[test]
    fn trust_then_login_keeps_one_answer_and_one_kind_notice() {
        let mut book = PromptChainBook::default();
        let key = key("trust-login");
        assert_eq!(book.admit(&key, "trust".into()), Admission::Allowed);
        assert!(book.is_active(&key));
        assert!(book.blocks_keys(&key));
        book.finish(&key);
        assert_eq!(book.entries[&key].answers, 1);
        assert!(book.claim_notice(&key, "login"));
        assert!(!book.claim_notice(&key, "login"));
        assert!(book.blocks_keys(&key));
    }

    #[test]
    fn trust_onboarding_ready_counts_two_answers_and_stops_observing() {
        let mut book = PromptChainBook::default();
        let key = key("trust-onboarding-ready");
        assert_eq!(book.admit(&key, "trust".into()), Admission::Allowed);
        assert_eq!(book.admit(&key, "onboarding".into()), Admission::Allowed);
        assert_eq!(book.entries[&key].answers, 2);
        book.finish(&key);
        assert!(!book.is_active(&key));
    }

    #[test]
    fn repeated_prompt_is_not_answered_again_and_five_is_the_limit() {
        let mut repeated = PromptChainBook::default();
        let repeated_key = key("repeat");
        assert_eq!(repeated.admit(&repeated_key, "same".into()), Admission::Allowed);
        assert_eq!(repeated.admit(&repeated_key, "same".into()), Admission::Repeated);
        assert_eq!(repeated.entries[&repeated_key].answers, 1);

        let mut bounded = PromptChainBook::default();
        let bounded_key = key("bounded");
        for index in 0..MAX_AUTOMATIC_ANSWERS {
            assert_eq!(
                bounded.admit(&bounded_key, format!("prompt-{index}")),
                Admission::Allowed
            );
        }
        assert_eq!(
            bounded.admit(&bounded_key, "sixth".into()),
            Admission::Limit
        );
        assert_eq!(bounded.entries[&bounded_key].answers, MAX_AUTOMATIC_ANSWERS);
    }

    #[test]
    fn cron_blocker_notice_uses_the_launch_title() {
        let body = notice_body("cron:Nightly QA", "login");
        assert!(body.contains("Nightly QA"));
        assert!(body.contains("login"));
    }

    #[test]
    fn unsupported_prompt_shapes_have_owner_blocker_kinds() {
        let mut prompt = PendingPrompt {
            question: "opaque-question".into(),
            options: Vec::new(),
            sections: None,
            multi_select: Some(false),
            submit_available: None,
            selected_values: None,
        };
        assert_eq!(blocker_kind(&prompt), "free_text");
        prompt.options = (0..11)
            .map(|index| codeterm_types::pty::PromptOption {
                label: format!("option-{index}"),
                value: format!("value-{index}"),
            })
            .collect();
        assert_eq!(blocker_kind(&prompt), "too_many_options");
        prompt.multi_select = Some(true);
        assert_eq!(blocker_kind(&prompt), "multi_select");
    }
}
