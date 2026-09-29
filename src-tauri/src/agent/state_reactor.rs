//! Generic, provider-agnostic tab-state reactor. A parked tab (`Idle`/`Waiting`)
//! is classified by its own provider; a verdict that clears [`GLOBAL_THRESHOLD`],
//! isn't [`TabState::WaitingOnAuth`], and carries an action is executed via key
//! injection / delayed-send / a user-facing surface. Core branches only on the
//! generic [`TabState`] / [`TabStateAction`], never a provider id or screen string.

use std::collections::HashSet;
use std::time::Duration;

use super::status::TabStatus;
use super::tab_state::{
    Surface, TabState, TabStateAction, TabStateClassification, TabStateSnapshot,
};
use crate::service::ServiceState;

/// Minimum confidence an actionable verdict must clear — the no-false-positive gate.
pub const GLOBAL_THRESHOLD: f32 = 0.8;

pub(crate) fn is_manual_update_prompt(classification: Option<&TabStateClassification>) -> bool {
    classification.is_some_and(|value| {
        value.state == TabState::UpdatePrompt && value.confidence >= GLOBAL_THRESHOLD
    })
}

/// How often the reactor sweeps tracked tabs for the parked edge.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Rows of screen tail handed to the classifier.
const SNAPSHOT_ROWS: usize = 60;

/// What the reactor decided. Pure output of [`decide`] so the gate/threshold/auth-guard are unit-testable.
#[derive(Debug, Clone, PartialEq)]
pub enum ReactorDecision {
    /// Do nothing (not parked, below threshold, no action, or unknown).
    Ignore,
    /// Present to the user only — never press a key (auth guard + surface-only verdicts).
    Surface(Option<Surface>),
    /// High-confidence, non-auth, actionable verdict — execute it.
    Act(TabStateAction),
}

/// Pure decision: gate on parked status, confidence threshold, and the auth guard. No side effects, no locks.
pub fn decide(
    status: TabStatus,
    classification: Option<&TabStateClassification>,
) -> ReactorDecision {
    // Gate: only act on a parked tab, never while it is Working/Dead.
    if !matches!(status, TabStatus::Idle | TabStatus::Waiting) {
        return ReactorDecision::Ignore;
    }
    let Some(c) = classification else {
        return ReactorDecision::Ignore;
    };
    // Threshold: a weak verdict never acts.
    if c.confidence < GLOBAL_THRESHOLD {
        return ReactorDecision::Ignore;
    }
    // Auth guard: a login prompt is never auto-advanced — surface only, even if keys were attached.
    if c.state == TabState::WaitingOnAuth {
        return ReactorDecision::Surface(c.action.as_ref().and_then(|a| a.surface.clone()));
    }
    if c.state == TabState::UpdatePrompt {
        return ReactorDecision::Ignore;
    }
    match &c.action {
        Some(action) => ReactorDecision::Act(action.clone()),
        None => ReactorDecision::Ignore,
    }
}

/// Translate an action's `keys` into ordered PTY byte writes; unknown keys are skipped (never the wrong byte).
pub fn action_to_writes(action: &TabStateAction) -> Vec<Vec<u8>> {
    action
        .keys
        .as_deref()
        .map(keys_to_writes)
        .unwrap_or_default()
}

fn keys_to_writes(keys: &[String]) -> Vec<Vec<u8>> {
    keys.iter()
        .filter_map(|key| super::keys::action_key_bytes(key))
        .collect()
}

/// Start the background reactor loop on its own thread: sweeps tracked tabs,
/// dedupes per parked episode, and executes verdicts off the async runtime.
pub fn start(svc: ServiceState) {
    std::thread::spawn(move || {
        // Tabs already reacted-to this parked episode — prevents re-firing every tick at the same prompt.
        let mut reacted: HashSet<String> = HashSet::new();
        loop {
            std::thread::sleep(POLL_INTERVAL);
            // Snapshot ids + statuses under one lock acquisition per tick (not N+1).
            let snapshot: Vec<(String, TabStatus)> = {
                let tracker = svc.agent_tracker.lock();
                tracker
                    .all_ids()
                    .into_iter()
                    .map(|id| {
                        let status = tracker.get_status(&id);
                        (id, status)
                    })
                    .collect()
            };
            for (pty_id, status) in &snapshot {
                match status {
                    TabStatus::Idle | TabStatus::Waiting => {
                        if reacted.insert(pty_id.clone()) {
                            react(&svc, pty_id, *status);
                        }
                    }
                    _ => {
                        reacted.remove(pty_id);
                    }
                }
            }
            reacted.retain(|id| snapshot.iter().any(|(sid, _)| sid == id));
        }
    });
}

/// Classify one parked tab and execute the verdict. Locks in CLAUDE.md order, each
/// scoped, never held across the provider call or a thread spawn.
fn react(svc: &ServiceState, pty_id: &str, status: TabStatus) {
    // mux (1): resolve the tab's OWN identity + provider + session/cwd.
    let (tab_id, agent_type, session_id, cwd, auto_approve, permission_auto_confirm) = {
        let mux = svc.mux.read();
        let Some(tab_id) = mux.tab_id_for_pty(pty_id) else {
            return;
        };
        let Some(tab) = mux.get_tab(&tab_id) else {
            return;
        };
        let Some(agent_type) = tab.agent_type.clone() else {
            return; // not an agent tab — nothing to classify
        };
        (
            tab_id,
            agent_type,
            tab.agent_session_id.clone(),
            tab.cwd.clone(),
            tab.auto_approve,
            tab.permission_auto_confirm,
        )
    };

    // screen_state (3): the rendered tail the classifier fingerprints against.
    let screen_text = svc
        .screen_state
        .lock()
        .tail_text(pty_id, SNAPSHOT_ROWS)
        .unwrap_or_default();

    let snapshot = TabStateSnapshot {
        tab_id: tab_id.clone(),
        agent_type: agent_type.clone(),
        session_id,
        cwd,
        screen_text,
    };

    // Ask the tab's OWN provider. No lock held across this call.
    let classification = svc
        .agent_registry
        .with_provider(&agent_type, |provider| {
            provider.classify_tab_state(&snapshot)
        })
        .unwrap_or(None);

    match decide(status, classification.as_ref()) {
        ReactorDecision::Ignore => {}
        ReactorDecision::Surface(surface) => surface_to_user(svc, &tab_id, surface),
        ReactorDecision::Act(action) => {
            if !route_classified_prompt(
                svc,
                &tab_id,
                &agent_type,
                &snapshot.screen_text,
                classification.as_ref(),
                auto_approve,
                permission_auto_confirm,
            ) {
                execute_action(svc, &tab_id, action);
            }
        }
    }
}

fn route_classified_prompt(
    svc: &ServiceState,
    tab_id: &str,
    agent_type: &str,
    screen: &str,
    classification: Option<&TabStateClassification>,
    auto_approve: bool,
    permission_auto_confirm: bool,
) -> bool {
    if !classification.is_some_and(|value| value.state == TabState::ClarifyingQuestion) {
        return false;
    }
    let parsed = crate::agent::prompt::parse_prompt_safely(|| {
        svc.agent_registry
            .parse_interactive_prompt(agent_type, screen)
    });
    let Some((prompt, prompt_kind)) = parsed else {
        let surface = classification
            .and_then(|value| value.action.as_ref())
            .and_then(|action| action.surface.clone());
        surface_to_user(svc, tab_id, surface);
        return true;
    };
    if !has_structural_prompt_evidence(screen, &prompt) {
        return true;
    }
    if auto_approve || permission_auto_confirm {
        let directory_trust_prompt = prompt_kind == codeterm_types::pty::PromptKind::DirectoryTrust
            || svc
                .agent_registry
                .with_provider(agent_type, |provider| {
                    provider.directory_trust_prompt_response(screen)
                })
                .flatten()
                .is_some();
        if !directory_trust_prompt {
            return true;
        }
    }
    let decision = svc.prompt_detector.lock().evaluate_pty_output_parsed(
        tab_id,
        true,
        true,
        Some(crate::agent::prompt::ResolvedPrompt {
            prompt,
            explicit: false,
        }),
    );
    svc.apply_prompt_decision_with_kind(&decision, Some(screen), prompt_kind, true);
    true
}

pub(crate) fn has_structural_prompt_evidence(
    screen: &str,
    prompt: &crate::multiplexer::types::PendingPrompt,
) -> bool {
    let lines = screen.lines().collect::<Vec<_>>();
    let last_option_row = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line
                .trim()
                .trim_start_matches(|ch| ch == '│' || ch == '┃')
                .trim();
            prompt
                .options
                .iter()
                .any(|option| line_has_option(line, option))
                .then_some(index)
        })
        .max();
    let Some(last_option_row) = last_option_row else {
        return false;
    };
    if !has_terminal_prompt_footer(&lines, last_option_row) {
        return false;
    }
    if lines[last_option_row + 1..].iter().any(|line| {
        matches!(
            line.trim()
                .trim_start_matches(|ch| ch == '│' || ch == '┃')
                .trim(),
            "❯" | "›" | "▶" | "➤" | ">"
        )
    }) {
        return false;
    }
    if prompt
        .selected_values
        .as_ref()
        .is_some_and(|selected| !selected.is_empty())
    {
        return true;
    }
    lines.iter().any(|line| {
        let line = line
            .trim()
            .trim_start_matches(|ch| ch == '│' || ch == '┃')
            .trim_start();
        let Some((marker, rest)) = line
            .char_indices()
            .next()
            .map(|(i, ch)| (ch, &line[i + ch.len_utf8()..]))
        else {
            return false;
        };
        if !matches!(marker, '❯' | '›' | '▶' | '➤' | '>') {
            return false;
        }
        let rest = rest.trim_start();
        prompt
            .options
            .iter()
            .any(|option| line_has_option(rest, option))
    })
}

fn has_terminal_prompt_footer(lines: &[&str], last_option_row: usize) -> bool {
    let footer_row = lines
        .iter()
        .enumerate()
        .skip(last_option_row + 1)
        .filter(|(_, line)| {
            let line = line.to_ascii_lowercase();
            line.contains("enter")
                && ["confirm", "continue", "select", "cancel", "go back"]
                    .iter()
                    .any(|action| line.contains(action))
        })
        .map(|(index, _)| index)
        .next_back();
    let Some(footer_row) = footer_row else {
        return false;
    };
    lines
        .iter()
        .enumerate()
        .skip(footer_row + 1)
        .all(|(_, line)| line.trim().is_empty())
}

/// A live composer row (a marker line that is not an option) below the options means the
/// "options" are transcript text, not an active dialog.
const COMPOSER_MARKERS: [char; 4] = ['❯', '›', '▶', '➤'];

pub(crate) fn prompt_is_composer_text(
    screen: &str,
    prompt: &crate::multiplexer::types::PendingPrompt,
) -> bool {
    question_is_composer_line(prompt) || composer_follows_options(screen, prompt)
}

fn question_is_composer_line(prompt: &crate::multiplexer::types::PendingPrompt) -> bool {
    let question = prompt.question.trim();
    question.starts_with(COMPOSER_MARKERS)
        && !prompt
            .options
            .iter()
            .any(|option| line_has_option(question.trim_start_matches(COMPOSER_MARKERS), option))
}

fn composer_follows_options(
    screen: &str,
    prompt: &crate::multiplexer::types::PendingPrompt,
) -> bool {
    let lines = screen
        .lines()
        .map(|line| line.trim().trim_start_matches(['│', '┃']).trim())
        .collect::<Vec<_>>();
    let is_option = |line: &str| {
        prompt
            .options
            .iter()
            .any(|option| line_has_option(line.trim_start_matches(COMPOSER_MARKERS), option))
    };
    let is_composer_line = |line: &str| line.starts_with(COMPOSER_MARKERS) && !is_option(line);
    let Some(last_option_row) = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_option(line))
        .map(|(index, _)| index)
        .max()
    else {
        return false;
    };
    // Numbered rows typed into the composer sit under its marker line, not under a question.
    let heads_composer_block = lines[..last_option_row]
        .iter()
        .rev()
        .find(|line| !line.is_empty() && !is_option(line))
        .is_some_and(|line| is_composer_line(line));
    heads_composer_block
        || lines[last_option_row + 1..]
            .iter()
            .any(|line| is_composer_line(line))
}

fn line_has_option(line: &str, option: &crate::multiplexer::types::PromptOption) -> bool {
    let indexed = line
        .trim_start()
        .strip_prefix(option.value.as_str())
        .is_some_and(|tail| {
            let tail = tail.trim_start();
            tail.starts_with('.') || tail.starts_with(')')
        });
    indexed || line.trim_start().starts_with(option.label.trim())
}

fn execute_action(svc: &ServiceState, tab_id: &str, action: TabStateAction) {
    let writes = action_to_writes(&action);
    if !writes.is_empty() {
        inject_keys(svc, tab_id, writes);
    }
    if let Some(resume) = &action.delayed_resume {
        schedule_resume(svc, tab_id, resume);
    }
    if let Some(surface) = action.surface {
        surface_to_user(svc, tab_id, Some(surface));
    }
}

/// Inject ordered key bytes into the tab's PTY (the `send --actions` path), off-thread so the poll loop never blocks.
fn inject_keys(svc: &ServiceState, tab_id: &str, writes: Vec<Vec<u8>>) {
    if svc.inject_raw_keys_async(tab_id, writes).is_err() {
        return;
    }
    log::info!("[state-reactor] injected keys into Tab {}", tab_id);
}

/// Schedule a resume via delayed-send at the parsed ISO time; an unparseable time is dropped, never fired at a guessed moment.
fn schedule_resume(svc: &ServiceState, tab_id: &str, resume: &super::tab_state::DelayedResume) {
    let Some(at_ms) = parse_iso_to_ms(&resume.at) else {
        log::warn!(
            "[state-reactor] unparseable resume time {:?} for Tab {} — skipping",
            resume.at,
            tab_id
        );
        return;
    };
    match crate::delayed_send::create_message(
        svc,
        tab_id.to_string(),
        resume.text.clone(),
        Some(at_ms),
        None,
        None,
    ) {
        Ok(_) => log::info!(
            "[state-reactor] scheduled resume for Tab {} at {}",
            tab_id,
            resume.at
        ),
        Err(e) => log::warn!("[state-reactor] schedule resume failed: {e}"),
    }
}

/// Surface a verdict to the user as a chat-card event.
fn surface_to_user(svc: &ServiceState, tab_id: &str, surface: Option<Surface>) {
    let (kind, payload) = match &surface {
        Some(s) => (s.kind.clone(), s.payload.clone()),
        None => ("tab_state".to_string(), serde_json::Value::Null),
    };
    crate::api::events::emit_everywhere(
        &svc.app_handle,
        "tab-state-surface",
        serde_json::json!({ "tab_id": tab_id, "kind": kind, "payload": payload }),
    );
    log::info!("[state-reactor] surfaced {} for Tab {}", kind, tab_id);
}

fn parse_iso_to_ms(iso: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.timestamp_millis())
        .filter(|ms| *ms >= 0)
        .map(|ms| ms as u64)
}

#[cfg(test)]
mod tests;
