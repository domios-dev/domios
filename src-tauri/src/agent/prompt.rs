//! PromptDetector — state machine for per-pane prompt lifecycle.
//!
//! Owns per-pane prompt state (Idle → Active → Cooldown → Idle).
//! Does NOT hold any external locks — takes data by value, returns decisions.

use std::collections::HashMap;
use std::time::Instant;

use crate::multiplexer::types::{PendingPrompt, PromptOption};

#[derive(Debug)]
enum PanePromptState {
    Idle,
    Active {
        prompt: PendingPrompt,
        #[allow(dead_code)]
        detected_at: Instant,
    },
    /// Suppress re-detection for cooldown_ms after user responds.
    Cooldown {
        responded_at: Instant,
    },
}

#[derive(Debug, Clone)]
pub enum PromptDecision {
    None,
    NewPrompt {
        pane_id: String,
        prompt: PendingPrompt,
    },
    PromptUpdated {
        pane_id: String,
        prompt: PendingPrompt,
    },
    /// Prompt disappeared from screen (user answered in terminal) — clear pending_prompt.
    PromptCleared {
        pane_id: String,
    },
}

/// Centralized prompt detection state machine.
/// Keyed by pane_id.
pub struct PromptDetector {
    states: HashMap<String, PanePromptState>,
    cooldown_ms: u64,
}

impl PromptDetector {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            cooldown_ms: 500,
        }
    }

    /// Main entry point for PTY output path.
    ///
    /// - `pane_id`: pane that produced output
    /// - `has_markers`: quick check result — if false, skip expensive parsing
    /// - `clean_ring_text`: cleaned ring buffer text (only provided when has_markers)
    /// - `parse_fn`: closure that parses prompt from text, returns (question, options)
    pub fn evaluate_pty_output<F>(
        &mut self,
        pane_id: &str,
        has_markers: bool,
        clean_ring_text: Option<&str>,
        parse_fn: F,
    ) -> PromptDecision
    where
        F: FnOnce(&str) -> Option<(String, Vec<(String, String)>)>,
    {
        // If pane has active prompt, always check screen (user may have answered in terminal)
        let is_active = matches!(
            self.states.get(pane_id),
            Some(PanePromptState::Active { .. })
        );

        if !has_markers && !is_active {
            return PromptDecision::None;
        }

        let text = match clean_ring_text {
            Some(t) => t,
            None => return PromptDecision::None,
        };

        self.detect_and_transition(pane_id, text, parse_fn)
    }

    /// Entry point for hook path (API endpoint).
    /// When an explicit PendingPrompt is provided, bypass parsing.
    pub fn hook_detected_explicit(
        &mut self,
        pane_id: &str,
        prompt: PendingPrompt,
    ) -> PromptDecision {
        // Hooks are authoritative — skip cooldown check
        let state = self.states.get(pane_id);
        match state {
            Some(PanePromptState::Active {
                prompt: existing, ..
            }) => {
                if same_options(&existing.options, &prompt.options) {
                    return PromptDecision::None;
                }
                self.states.insert(
                    pane_id.to_string(),
                    PanePromptState::Active {
                        prompt: prompt.clone(),
                        detected_at: Instant::now(),
                    },
                );
                PromptDecision::PromptUpdated {
                    pane_id: pane_id.to_string(),
                    prompt,
                }
            }
            _ => {
                self.states.insert(
                    pane_id.to_string(),
                    PanePromptState::Active {
                        prompt: prompt.clone(),
                        detected_at: Instant::now(),
                    },
                );
                PromptDecision::NewPrompt {
                    pane_id: pane_id.to_string(),
                    prompt,
                }
            }
        }
    }

    /// Entry point for hook path that parses from ring buffer.
    pub fn hook_detected_parse<F>(
        &mut self,
        pane_id: &str,
        clean_ring_text: &str,
        parse_fn: F,
    ) -> PromptDecision
    where
        F: FnOnce(&str) -> Option<(String, Vec<(String, String)>)>,
    {
        // Hooks are authoritative — skip cooldown, always parse
        let parsed = parse_fn(clean_ring_text);
        log::info!(
            "[prompt] hook_detected_parse for {}: parsed={}, text_len={}",
            pane_id,
            parsed.is_some(),
            clean_ring_text.len()
        );
        if parsed.is_none() && clean_ring_text.len() > 100 {
            log::info!(
                "[prompt] screen tail: {:?}",
                &clean_ring_text[clean_ring_text.len().saturating_sub(300)..]
            );
        }
        match parsed {
            Some((question, opts)) => {
                log::info!(
                    "[prompt] parsed question={:?}, opts={}",
                    question,
                    opts.len()
                );
                let prompt = build_prompt(question, opts);
                self.hook_detected_explicit(pane_id, prompt)
            }
            None => PromptDecision::None,
        }
    }

    /// Called when prompt is cleared via API (test cleanup) — resets to Idle.
    pub fn clear(&mut self, pane_id: &str) {
        self.states.remove(pane_id);
    }

    /// Called when user clicks a prompt button — transitions to Cooldown.
    pub fn mark_responded(&mut self, pane_id: &str) {
        self.states.insert(
            pane_id.to_string(),
            PanePromptState::Cooldown {
                responded_at: Instant::now(),
            },
        );
    }

    /// Get the current active prompt for a pane (if any).
    #[allow(dead_code)]
    pub fn active_prompt(&self, pane_id: &str) -> Option<&PendingPrompt> {
        match self.states.get(pane_id) {
            Some(PanePromptState::Active { prompt, .. }) => Some(prompt),
            _ => None,
        }
    }

    /// Remove state for a closed pane.
    pub fn remove_pane(&mut self, pane_id: &str) {
        self.states.remove(pane_id);
    }

    fn detect_and_transition<F>(&mut self, pane_id: &str, text: &str, parse_fn: F) -> PromptDecision
    where
        F: FnOnce(&str) -> Option<(String, Vec<(String, String)>)>,
    {
        let state = self.states.get(pane_id);

        match state {
            Some(PanePromptState::Active {
                prompt: existing, ..
            }) => {
                match parse_fn(text) {
                    Some((question, opts)) => {
                        let new_prompt = build_prompt(question, opts);
                        if same_options(&existing.options, &new_prompt.options) {
                            PromptDecision::None
                        } else {
                            self.states.insert(
                                pane_id.to_string(),
                                PanePromptState::Active {
                                    prompt: new_prompt.clone(),
                                    detected_at: Instant::now(),
                                },
                            );
                            PromptDecision::PromptUpdated {
                                pane_id: pane_id.to_string(),
                                prompt: new_prompt,
                            }
                        }
                    }
                    None => {
                        // Prompt gone from screen — user answered in terminal
                        self.states
                            .insert(pane_id.to_string(), PanePromptState::Idle);
                        PromptDecision::PromptCleared {
                            pane_id: pane_id.to_string(),
                        }
                    }
                }
            }

            Some(PanePromptState::Cooldown { responded_at }) => {
                if responded_at.elapsed().as_millis() < self.cooldown_ms as u128 {
                    return PromptDecision::None;
                }
                self.states
                    .insert(pane_id.to_string(), PanePromptState::Idle);
                self.try_detect(pane_id, text, parse_fn)
            }

            Some(PanePromptState::Idle) | None => self.try_detect(pane_id, text, parse_fn),
        }
    }

    fn try_detect<F>(&mut self, pane_id: &str, text: &str, parse_fn: F) -> PromptDecision
    where
        F: FnOnce(&str) -> Option<(String, Vec<(String, String)>)>,
    {
        let result = parse_fn(text);
        match result {
            Some((question, opts)) => {
                let prompt = build_prompt(question, opts);
                self.states.insert(
                    pane_id.to_string(),
                    PanePromptState::Active {
                        prompt: prompt.clone(),
                        detected_at: Instant::now(),
                    },
                );
                PromptDecision::NewPrompt {
                    pane_id: pane_id.to_string(),
                    prompt,
                }
            }
            None => PromptDecision::None,
        }
    }
}

/// Build a PendingPrompt from parsed (question, options) tuple.
fn build_prompt(question: String, opts: Vec<(String, String)>) -> PendingPrompt {
    PendingPrompt {
        question,
        options: opts
            .into_iter()
            .map(|(label, value)| PromptOption { label, value })
            .collect(),
    }
}

/// Check if two option lists have the same values (order-sensitive).
fn same_options(a: &[PromptOption], b: &[PromptOption]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.value == y.value && x.label == y.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_prompt(q: &str, opts: &[(&str, &str)]) -> PendingPrompt {
        PendingPrompt {
            question: q.to_string(),
            options: opts
                .iter()
                .map(|(l, v)| PromptOption {
                    label: l.to_string(),
                    value: v.to_string(),
                })
                .collect(),
        }
    }

    fn parse_yes_no(text: &str) -> Option<(String, Vec<(String, String)>)> {
        if text.contains("proceed") {
            Some((
                "Do you want to proceed?".to_string(),
                vec![
                    ("Yes".to_string(), "1".to_string()),
                    ("No".to_string(), "2".to_string()),
                ],
            ))
        } else {
            None
        }
    }

    fn parse_three_opts(text: &str) -> Option<(String, Vec<(String, String)>)> {
        if text.contains("proceed") {
            Some((
                "Do you want to proceed?".to_string(),
                vec![
                    ("Yes".to_string(), "1".to_string()),
                    ("Yes, always".to_string(), "2".to_string()),
                    ("No".to_string(), "3".to_string()),
                ],
            ))
        } else {
            None
        }
    }

    #[test]
    fn test_idle_to_active() {
        let mut det = PromptDetector::new();
        let decision = det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
        if let PromptDecision::NewPrompt { pane_id, prompt } = &decision {
            assert_eq!(pane_id, "pane-1");
            assert_eq!(prompt.options.len(), 2);
        }
    }

    #[test]
    fn test_active_same_prompt_returns_none() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        // Same prompt again
        let decision = det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_active_different_options_returns_updated() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        // Different options
        let decision = det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_three_opts);
        assert!(matches!(decision, PromptDecision::PromptUpdated { .. }));
    }

    #[test]
    fn test_no_markers_returns_none() {
        let mut det = PromptDetector::new();
        let decision = det.evaluate_pty_output("pane-1", false, None, parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_cooldown_suppresses_detection() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        det.mark_responded("pane-1");
        // Immediately after response — should be suppressed
        let decision = det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_cooldown_expires() {
        let mut det = PromptDetector::new();
        det.cooldown_ms = 0; // Expire immediately
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        det.mark_responded("pane-1");
        // Cooldown expired (0ms) — should detect new prompt
        let decision = det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_hook_explicit_new_prompt() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Proceed?", &[("Yes", "1"), ("No", "2")]);
        let decision = det.hook_detected_explicit("pane-1", prompt);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_hook_explicit_same_prompt_returns_none() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Proceed?", &[("Yes", "1"), ("No", "2")]);
        det.hook_detected_explicit("pane-1", prompt.clone());
        let decision = det.hook_detected_explicit("pane-1", prompt);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_hook_bypasses_cooldown() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Proceed?", &[("Yes", "1"), ("No", "2")]);
        det.hook_detected_explicit("pane-1", prompt.clone());
        det.mark_responded("pane-1");
        // Hook should bypass cooldown
        let prompt2 = make_prompt("Allow?", &[("Yes", "1"), ("No", "2")]);
        let decision = det.hook_detected_explicit("pane-1", prompt2);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_remove_pane() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        det.remove_pane("pane-1");
        assert!(det.active_prompt("pane-1").is_none());
    }

    #[test]
    fn test_no_parse_result_stays_idle() {
        let mut det = PromptDetector::new();
        let decision = det.evaluate_pty_output("pane-1", true, Some("nothing here"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_active_prompt_cleared_when_gone() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        // Prompt disappears from screen (user answered in terminal)
        let decision =
            det.evaluate_pty_output("pane-1", true, Some("no prompt here"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::PromptCleared { .. }));
        assert!(det.active_prompt("pane-1").is_none());
    }

    #[test]
    fn test_hook_parse_from_ring_buffer() {
        let mut det = PromptDetector::new();
        let decision = det.hook_detected_parse("pane-1", "proceed", parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_hook_parse_no_prompt_returns_none() {
        let mut det = PromptDetector::new();
        let decision = det.hook_detected_parse("pane-1", "nothing", parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_multiple_panes_independent() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("pane-1", true, Some("proceed"), parse_yes_no);
        det.mark_responded("pane-1");
        // pane-2 should not be affected by pane-1's cooldown
        let decision = det.evaluate_pty_output("pane-2", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }
}
