//! PromptDetector — state machine for per-tab prompt lifecycle.
//!
//! Owns per-tab prompt state (Idle → Active → Cooldown → Idle).
//! Does NOT hold any external locks — takes data by value, returns decisions.

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

use crate::multiplexer::types::{PendingPrompt, PromptOption};

#[derive(Debug)]
enum TabPromptState {
    Idle,
    Active {
        prompt: PendingPrompt,
        #[allow(dead_code)]
        detected_at: Instant,
        /// Set by a hook call with no live structured backing (explicit JSON
        /// body, or screen-parsed hook fallback) rather than the continuous
        /// structured-handle scan. Such prompts have no per-chunk source to
        /// reconcile against, so a routine "nothing parsed this chunk" result
        /// must not clear them — only `/prompt/clear` or `hook_stop` does.
        explicit: bool,
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
        tab_id: String,
        prompt: PendingPrompt,
    },
    PromptUpdated {
        tab_id: String,
        prompt: PendingPrompt,
    },
    /// Prompt disappeared from screen (user answered in terminal) — clear pending_prompt.
    PromptCleared {
        tab_id: String,
    },
}

pub type ParsedPrompt = Option<PendingPrompt>;

/// Centralized prompt detection state machine.
/// Keyed by tab_id.
pub struct PromptDetector {
    states: HashMap<String, TabPromptState>,
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
    /// - `tab_id`: tab that produced output
    /// - `has_markers`: quick check result — if false, skip expensive parsing
    /// - `clean_ring_text`: cleaned ring buffer text (only provided when has_markers)
    /// - `parse_fn`: closure that parses prompt from text, returns (question, options)
    pub fn evaluate_pty_output<F>(
        &mut self,
        tab_id: &str,
        has_markers: bool,
        clean_ring_text: Option<&str>,
        parse_fn: F,
    ) -> PromptDecision
    where
        F: FnOnce(&str) -> ParsedPrompt,
    {
        // If tab has active prompt, always check screen (user may have answered in terminal)
        let is_active = matches!(self.states.get(tab_id), Some(TabPromptState::Active { .. }));

        if !has_markers && !is_active {
            return PromptDecision::None;
        }

        let text = match clean_ring_text {
            Some(t) => t,
            None => return PromptDecision::None,
        };

        let parsed = parse_prompt_safely(|| parse_fn(text));
        self.transition_from_parsed(tab_id, parsed)
    }

    pub fn evaluate_pty_output_parsed(
        &mut self,
        tab_id: &str,
        has_markers: bool,
        clean_ring_text_present: bool,
        parsed: ParsedPrompt,
    ) -> PromptDecision {
        let is_active = matches!(self.states.get(tab_id), Some(TabPromptState::Active { .. }));

        if !has_markers && !is_active {
            return PromptDecision::None;
        }

        if !clean_ring_text_present {
            return PromptDecision::None;
        }

        self.transition_from_parsed(tab_id, parsed)
    }

    /// Entry point for hook path (API endpoint).
    /// When an explicit PendingPrompt is provided, bypass parsing.
    pub fn hook_detected_explicit(
        &mut self,
        tab_id: &str,
        prompt: PendingPrompt,
    ) -> PromptDecision {
        let state = self.states.get(tab_id);
        match state {
            Some(TabPromptState::Active {
                prompt: existing, ..
            }) => {
                if same_prompt(existing, &prompt) {
                    return PromptDecision::None;
                }
                self.states.insert(
                    tab_id.to_string(),
                    TabPromptState::Active {
                        prompt: prompt.clone(),
                        detected_at: Instant::now(),
                        explicit: true,
                    },
                );
                PromptDecision::PromptUpdated {
                    tab_id: tab_id.to_string(),
                    prompt,
                }
            }
            _ => {
                self.states.insert(
                    tab_id.to_string(),
                    TabPromptState::Active {
                        prompt: prompt.clone(),
                        detected_at: Instant::now(),
                        explicit: true,
                    },
                );
                PromptDecision::NewPrompt {
                    tab_id: tab_id.to_string(),
                    prompt,
                }
            }
        }
    }

    /// Entry point for hook path that parses from ring buffer.
    pub fn hook_detected_parse<F>(
        &mut self,
        tab_id: &str,
        clean_ring_text: &str,
        parse_fn: F,
    ) -> PromptDecision
    where
        F: FnOnce(&str) -> ParsedPrompt,
    {
        let parsed = parse_hook_prompt(clean_ring_text, parse_fn);
        self.hook_detected_parsed(
            tab_id,
            clean_ring_text.len(),
            Some(utf8_tail(clean_ring_text, 300)),
            parsed,
        )
    }

    /// Called when prompt is cleared via API (test cleanup) — resets to Idle.
    pub fn clear(&mut self, tab_id: &str) {
        self.states.remove(tab_id);
    }

    pub fn hook_detected_parsed(
        &mut self,
        tab_id: &str,
        text_len: usize,
        text_tail: Option<&str>,
        parsed: ParsedPrompt,
    ) -> PromptDecision {
        log::info!(
            "[prompt] hook_detected_parse for {}: parsed={}, text_len={}",
            tab_id,
            parsed.is_some(),
            text_len
        );
        if parsed.is_none() && text_len > 100 {
            if let Some(tail) = text_tail {
                log::info!("[prompt] screen tail: {:?}", tail);
            }
        }
        match parsed {
            Some(prompt) => {
                log::info!(
                    "[prompt] parsed question={:?}, opts={}",
                    prompt.question,
                    prompt.options.len()
                );
                self.hook_detected_explicit(tab_id, prompt)
            }
            None => PromptDecision::None,
        }
    }

    /// Called when user clicks a prompt button — transitions to Cooldown.
    pub fn mark_responded(&mut self, tab_id: &str) {
        self.states.insert(
            tab_id.to_string(),
            TabPromptState::Cooldown {
                responded_at: Instant::now(),
            },
        );
    }

    /// Get the current active prompt for a tab (if any).
    #[allow(dead_code)]
    pub fn active_prompt(&self, tab_id: &str) -> Option<&PendingPrompt> {
        match self.states.get(tab_id) {
            Some(TabPromptState::Active { prompt, .. }) => Some(prompt),
            _ => None,
        }
    }

    /// Remove state for a closed tab.
    pub fn remove_tab(&mut self, tab_id: &str) {
        self.states.remove(tab_id);
    }

    fn transition_from_parsed(&mut self, tab_id: &str, parsed: ParsedPrompt) -> PromptDecision {
        match self.states.get(tab_id) {
            Some(TabPromptState::Active {
                prompt: existing,
                explicit,
                ..
            }) => {
                let explicit = *explicit;
                match parsed {
                    Some(new_prompt) => {
                        if same_prompt(existing, &new_prompt) {
                            PromptDecision::None
                        } else {
                            self.states.insert(
                                tab_id.to_string(),
                                TabPromptState::Active {
                                    prompt: new_prompt.clone(),
                                    detected_at: Instant::now(),
                                    explicit: false,
                                },
                            );
                            PromptDecision::PromptUpdated {
                                tab_id: tab_id.to_string(),
                                prompt: new_prompt,
                            }
                        }
                    }
                    None if explicit => PromptDecision::None,
                    None => {
                        self.states.insert(tab_id.to_string(), TabPromptState::Idle);
                        PromptDecision::PromptCleared {
                            tab_id: tab_id.to_string(),
                        }
                    }
                }
            }

            Some(TabPromptState::Cooldown { responded_at }) => {
                if responded_at.elapsed().as_millis() < self.cooldown_ms as u128 {
                    return PromptDecision::None;
                }
                self.states.insert(tab_id.to_string(), TabPromptState::Idle);
                self.try_apply_parsed(tab_id, parsed)
            }

            Some(TabPromptState::Idle) | None => self.try_apply_parsed(tab_id, parsed),
        }
    }

    fn try_apply_parsed(&mut self, tab_id: &str, parsed: ParsedPrompt) -> PromptDecision {
        match parsed {
            Some(prompt) => {
                self.states.insert(
                    tab_id.to_string(),
                    TabPromptState::Active {
                        prompt: prompt.clone(),
                        detected_at: Instant::now(),
                        explicit: false,
                    },
                );
                PromptDecision::NewPrompt {
                    tab_id: tab_id.to_string(),
                    prompt,
                }
            }
            None => PromptDecision::None,
        }
    }
}

pub(crate) fn utf8_tail(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut start = text.len() - max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

fn parse_hook_prompt<F>(clean_ring_text: &str, parse_fn: F) -> ParsedPrompt
where
    F: FnOnce(&str) -> ParsedPrompt,
{
    parse_prompt_safely(|| parse_fn(clean_ring_text))
}

pub(crate) fn parse_prompt_safely<F>(parse_fn: F) -> ParsedPrompt
where
    F: FnOnce() -> ParsedPrompt,
{
    match catch_unwind(AssertUnwindSafe(parse_fn)) {
        Ok(parsed) => parsed,
        Err(_) => {
            log::error!("[prompt] parser panicked; ignoring prompt detection");
            None
        }
    }
}

fn same_prompt(a: &PendingPrompt, b: &PendingPrompt) -> bool {
    a.question == b.question
        && same_options(&a.options, &b.options)
        && a.sections == b.sections
        && a.multi_select == b.multi_select
        && a.submit_available == b.submit_available
        && a.selected_values == b.selected_values
}

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
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    fn parse_yes_no(text: &str) -> ParsedPrompt {
        if text.contains("proceed") {
            Some(make_prompt(
                "Do you want to proceed?",
                &[("Yes", "1"), ("No", "2")],
            ))
        } else {
            None
        }
    }

    fn parse_three_opts(text: &str) -> ParsedPrompt {
        if text.contains("proceed") {
            Some(make_prompt(
                "Do you want to proceed?",
                &[("Yes", "1"), ("Yes, always", "2"), ("No", "3")],
            ))
        } else {
            None
        }
    }

    #[test]
    fn test_idle_to_active() {
        let mut det = PromptDetector::new();
        let decision = det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
        if let PromptDecision::NewPrompt { tab_id, prompt } = &decision {
            assert_eq!(tab_id, "tab-1");
            assert_eq!(prompt.options.len(), 2);
        }
    }

    #[test]
    fn test_active_same_prompt_returns_none() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        let decision = det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_active_different_options_returns_updated() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        let decision = det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_three_opts);
        assert!(matches!(decision, PromptDecision::PromptUpdated { .. }));
    }

    #[test]
    fn test_no_markers_returns_none() {
        let mut det = PromptDetector::new();
        let decision = det.evaluate_pty_output("tab-1", false, None, parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_cooldown_suppresses_detection() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        det.mark_responded("tab-1");
        let decision = det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_cooldown_expires() {
        let mut det = PromptDetector::new();
        det.cooldown_ms = 0; // Expire immediately
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        det.mark_responded("tab-1");
        let decision = det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_hook_explicit_new_prompt() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Proceed?", &[("Yes", "1"), ("No", "2")]);
        let decision = det.hook_detected_explicit("tab-1", prompt);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_hook_explicit_same_prompt_returns_none() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Proceed?", &[("Yes", "1"), ("No", "2")]);
        det.hook_detected_explicit("tab-1", prompt.clone());
        let decision = det.hook_detected_explicit("tab-1", prompt);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_hook_bypasses_cooldown() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Proceed?", &[("Yes", "1"), ("No", "2")]);
        det.hook_detected_explicit("tab-1", prompt.clone());
        det.mark_responded("tab-1");
        let prompt2 = make_prompt("Allow?", &[("Yes", "1"), ("No", "2")]);
        let decision = det.hook_detected_explicit("tab-1", prompt2);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_remove_tab() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        det.remove_tab("tab-1");
        assert!(det.active_prompt("tab-1").is_none());
    }

    #[test]
    fn test_no_parse_result_stays_idle() {
        let mut det = PromptDetector::new();
        let decision = det.evaluate_pty_output("tab-1", true, Some("nothing here"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_active_prompt_cleared_when_gone() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        // Prompt disappears from screen (user answered in terminal)
        let decision = det.evaluate_pty_output("tab-1", true, Some("no prompt here"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::PromptCleared { .. }));
        assert!(det.active_prompt("tab-1").is_none());
    }

    #[test]
    fn test_hook_parse_from_ring_buffer() {
        let mut det = PromptDetector::new();
        let decision = det.hook_detected_parse("tab-1", "proceed", parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_explicit_prompt_survives_unrelated_pty_output() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Allow probe?", &[("Yes", "1"), ("No", "2")]);
        det.hook_detected_explicit("tab-1", prompt);
        let decision = det.evaluate_pty_output("tab-1", true, Some("pw-rail-42"), |_| None);
        assert!(matches!(decision, PromptDecision::None));
        assert!(det.active_prompt("tab-1").is_some());
    }

    // The screen-parsed hook fallback (no JSON body — real agent hook firing)
    // is equally source-less per chunk, so it gets the same immunity.
    #[test]
    fn test_hook_parsed_prompt_survives_unrelated_pty_output() {
        let mut det = PromptDetector::new();
        det.hook_detected_parse("tab-1", "proceed", parse_yes_no);
        let decision = det.evaluate_pty_output("tab-1", true, Some("nothing here"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
        assert!(det.active_prompt("tab-1").is_some());
    }

    #[test]
    fn test_structured_prompt_still_clears_when_gone() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        let decision = det.evaluate_pty_output("tab-1", true, Some("no prompt here"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::PromptCleared { .. }));
        assert!(det.active_prompt("tab-1").is_none());
    }

    #[test]
    fn test_structured_prompt_supersedes_explicit() {
        let mut det = PromptDetector::new();
        let prompt = make_prompt("Allow probe?", &[("Yes", "1"), ("No", "2")]);
        det.hook_detected_explicit("tab-1", prompt);
        let decision = det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::PromptUpdated { .. }));
        let active = det.active_prompt("tab-1").expect("prompt still active");
        assert_eq!(active.question, "Do you want to proceed?");
    }

    #[test]
    fn test_hook_parse_no_prompt_returns_none() {
        let mut det = PromptDetector::new();
        let decision = det.hook_detected_parse("tab-1", "nothing", parse_yes_no);
        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_hook_parse_multibyte_tail_does_not_panic() {
        let mut det = PromptDetector::new();
        let text = format!("{}{}{}", "a".repeat(10), "─", "b".repeat(298));
        assert_eq!(
            text.len() - 300,
            11,
            "fixture must put old byte-slice start inside the box drawing char"
        );
        assert!(
            !text.is_char_boundary(text.len() - 300),
            "fixture must reproduce the invalid UTF-8 boundary"
        );

        let decision = det.hook_detected_parse("tab-1", &text, parse_yes_no);

        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_hook_parse_contains_parser_panic() {
        let mut det = PromptDetector::new();
        let decision = det.hook_detected_parse("tab-1", "│ emoji 😀 cjk 漢字", |_| {
            panic!("provider parser bug")
        });

        assert!(matches!(decision, PromptDecision::None));
    }

    #[test]
    fn test_parser_panic_does_not_strand_detector_lock() {
        let detector = parking_lot::Mutex::new(PromptDetector::new());
        let text = "│ emoji 😀 cjk 漢字";

        let parsed = parse_prompt_safely(|| {
            panic!("provider parser bug");
        });
        let decision =
            detector
                .lock()
                .hook_detected_parsed("tab-1", text.len(), Some(text), parsed);
        assert!(matches!(decision, PromptDecision::None));

        let parsed = parse_prompt_safely(|| parse_yes_no("proceed"));
        let decision =
            detector
                .lock()
                .hook_detected_parsed("tab-1", "proceed".len(), Some("proceed"), parsed);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }

    #[test]
    fn test_utf8_tail_starts_on_char_boundary() {
        let text = format!("{}{}{}", "a".repeat(10), "😀", "b".repeat(298));
        let old_start = text.len() - 300;
        assert!(!text.is_char_boundary(old_start));

        let tail = utf8_tail(&text, 300);

        assert!(tail.len() <= 300);
        assert!(tail.starts_with('b'));
    }

    #[test]
    fn test_multiple_tabs_independent() {
        let mut det = PromptDetector::new();
        det.evaluate_pty_output("tab-1", true, Some("proceed"), parse_yes_no);
        det.mark_responded("tab-1");
        let decision = det.evaluate_pty_output("tab-2", true, Some("proceed"), parse_yes_no);
        assert!(matches!(decision, PromptDecision::NewPrompt { .. }));
    }
}
