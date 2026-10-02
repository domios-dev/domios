use super::*;
use crate::agent::tab_state::TabStateSnapshot;
use crate::service::supporter_tab_state::{CandidateVerdict, PROVIDER_UPDATE_PROMPT};
use codeterm_types::pty::PromptKind;

impl ServiceContext {
    pub(crate) fn is_scrape_candidate(&self, agent: &str, screen: &str) -> bool {
        manual_update_prompt_candidate(screen)
            || trust_candidate::may_be_directory_trust_prompt(screen)
            || crate::agent::prompt::parse_prompt_safely(|| {
                self.agent_registry.parse_interactive_prompt(agent, screen)
            })
            .is_some()
    }

    pub(crate) fn observe_output_scrape_candidate(&self, snapshot: TabStateSnapshot) {
        let fallback_snapshot = snapshot.clone();
        self.observe_scrape_candidate(snapshot, move |service| {
            service.resolve_scrape_candidate_heuristically(&fallback_snapshot);
        });
    }

    fn resolve_scrape_candidate_heuristically(&self, snapshot: &TabStateSnapshot) {
        let onboarding = {
            let mux = self.mux.read();
            mux.get_tab(&snapshot.tab_id)
                .filter(|tab| {
                    super::super::directory_trust::launch_onboarding_check_applies(
                        tab,
                        &snapshot.agent_type,
                    )
                })
                .and_then(|tab| tab.pty_id.clone())
        };
        if let Some(pty) = onboarding.filter(|_| {
            self.agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    provider
                        .launch_onboarding_response(&snapshot.screen_text)
                        .is_some()
                })
                .unwrap_or(false)
        }) {
            self.enqueue_launch_onboarding_screen_check(
                &snapshot.tab_id,
                &pty,
                &snapshot.agent_type,
            );
            return;
        }
        let flags = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(&snapshot.tab_id) else {
                return;
            };
            (
                super::super::directory_trust::directory_trust_check_applies(tab),
                super::super::directory_trust::shared_directory_trust_check_applies(tab),
                tab.auto_approve,
                super::super::directory_trust::directory_trust_skip_reason(tab),
            )
        };
        let trust = trust_candidate::candidate_for_output(
            self,
            &snapshot.tab_id,
            &snapshot.agent_type,
            Some(&snapshot.screen_text),
            flags.0,
            flags.1,
            flags.2,
            flags.3,
        );
        if manual_update_prompt_candidate(&snapshot.screen_text) {
            if let Some((prompt, safe_choice)) = self
                .agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    parse_manual_update_prompt_with_response(provider, snapshot)
                })
                .flatten()
            {
                resolve_classified_manual_update_prompt(self, snapshot, prompt, safe_choice);
            }
            return;
        }
        let parsed = crate::agent::prompt::parse_prompt_safely(|| {
            self.agent_registry
                .parse_interactive_prompt(&snapshot.agent_type, &snapshot.screen_text)
        })
        .filter(|(prompt, kind)| {
            *kind == PromptKind::DirectoryTrust
                || !crate::agent::state_reactor::prompt_is_composer_text(
                    &snapshot.screen_text,
                    prompt,
                )
        });
        let kind = if trust {
            PromptKind::DirectoryTrust
        } else {
            parsed
                .as_ref()
                .map_or(PromptKind::Provider, |(_, kind)| *kind)
        };
        let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
            &snapshot.tab_id,
            true,
            true,
            parsed.map(|(prompt, _)| crate::agent::prompt::ResolvedPrompt {
                prompt,
                explicit: false,
            }),
        );
        self.apply_prompt_decision_with_kind(&decision, Some(&snapshot.screen_text), kind, false);
    }

    pub(crate) fn publish_model_scrape_candidate(
        &self,
        snapshot: &TabStateSnapshot,
        verdict: &CandidateVerdict,
        valid: &dyn Fn() -> bool,
    ) {
        if !valid()
            || self
                .prompt_detector
                .lock()
                .active_prompt_explicit(&snapshot.tab_id)
                == Some(true)
        {
            return;
        }
        let parsed = if verdict.live_prompt() {
            crate::agent::prompt::parse_prompt_safely(|| {
                self.agent_registry
                    .parse_interactive_prompt(&snapshot.agent_type, &snapshot.screen_text)
            })
            .filter(|(prompt, _)| {
                crate::service::supporter_tab_state::candidate_allows_prompt(
                    verdict,
                    &snapshot.screen_text,
                    prompt,
                )
            })
        } else {
            None
        };
        let kind = if verdict.label == PROVIDER_UPDATE_PROMPT {
            PromptKind::ManualUpdate
        } else {
            parsed
                .as_ref()
                .map_or(PromptKind::Provider, |(_, kind)| *kind)
        };
        if !valid() {
            return;
        }
        if parsed
            .as_ref()
            .is_some_and(|_| self.try_model_safe_response(snapshot, verdict))
        {
            return;
        }
        let decision = self.prompt_detector.lock().evaluate_pty_output_parsed(
            &snapshot.tab_id,
            true,
            true,
            parsed.map(|(prompt, _)| crate::agent::prompt::ResolvedPrompt {
                prompt,
                explicit: false,
            }),
        );
        self.apply_prompt_decision_immediately(
            &decision,
            Some(&snapshot.screen_text),
            kind,
            true,
            verdict.confidence >= 0.80,
        );
    }

    pub(super) fn classify_manual_update_prompt_off_runtime(
        &self,
        pty_id: String,
        snapshot: TabStateSnapshot,
    ) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let classified = svc
                .agent_registry
                .with_provider(&snapshot.agent_type, |provider| {
                    parse_manual_update_prompt_with_response(provider, &snapshot)
                })
                .flatten();
            let Some((prompt, skip_value)) = classified else {
                return;
            };
            let screen = svc.screen_state.lock().get_screen_text(&pty_id);
            if screen.as_deref() != Some(snapshot.screen_text.as_str()) {
                return;
            }
            resolve_classified_manual_update_prompt(&svc, &snapshot, prompt, skip_value);
        });
    }
}
