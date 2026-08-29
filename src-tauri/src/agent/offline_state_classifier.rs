use serde::{Deserialize, Serialize};

use super::attributed_screen::{detect_picker, PickerDetection};
use super::classify_input::{ClassifyInputBuilder, ClassifyParts, StatusBarRows};
use super::signal_extractor::{Motion, SignalExtractor, StreamSignals};

mod status;

use status::completion_summary_immediately_precedes_status_repaint;

pub use status::{
    extract_completion_summary, extract_status_line, has_active_turn_footer, StatusLine,
};

pub use codeterm_types::pty::{AgentLiveState, QuestionSubtype, TabVerdict, TimelineState};

pub const TICK_MS: u64 = 250;
pub const WAIT_MS: u64 = 800;
pub const DONE_MS: u64 = 800;
const WORKING_GRACE_MS: u64 = 1500;
const BOTTOM_REGION_ROWS: u16 = 8;
/// Upper bound on how long a Working verdict may persist with zero active-work
/// evidence and total PTY silence before resolving to Idle (anti-stuck-Working).
const STUCK_WORKING_IDLE_MS: u64 = 5000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedSegment {
    pub from_ms: u64,
    pub to_ms: u64,
    pub state: TimelineState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<QuestionSubtype>,
}

#[derive(Clone, Debug)]
pub enum CastEvent {
    Output { now_ms: u64, bytes: Vec<u8> },
    Resize { now_ms: u64, rows: u16, cols: u16 },
}

impl CastEvent {
    pub fn now_ms(&self) -> u64 {
        match self {
            CastEvent::Output { now_ms, .. } | CastEvent::Resize { now_ms, .. } => *now_ms,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CastTimelineInput {
    pub width: u16,
    pub height: u16,
    /// Required, never defaulted: the replayed provider's chrome band. Offline
    /// gates the semantic clock with exactly the band live excludes.
    pub status_bar_rows: StatusBarRows,
    pub events: Vec<CastEvent>,
}

/// Minimal per-tick inputs the 3-state engine classifies. Both paths build this
/// and call [`classify_state`]:
/// - offline replay fills `picker_question` by running `detect_picker` over the
///   replayed grid;
/// - the live PTY path passes the already-surfaced pending prompt's question so
///   it never re-parses the picker, and derives `screen_lines`/`tail` from the
///   existing per-tab `ScreenState` parser (no second vt100).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifyInput<'a> {
    /// Resolved provider chrome band. Required input, no default: it is built
    /// only via `classify_input::ClassifyInputBuilder`.
    pub status_bar_rows: StatusBarRows,
    /// Pre-detected picker question, if a picker is on screen.
    pub picker_question: Option<&'a str>,
    /// Trimmed per-row screen text (rate-limit notices + ready-prompt fingerprints).
    pub screen_lines: &'a [String],
    /// Recent screen tail (subtype fingerprints for a picker question).
    pub tail: &'a str,
    /// Two-clock quiescence: any byte resets `byte_idle`; only a non-volatile
    /// change resets `semantic_idle`.
    pub byte_idle_ms: u64,
    pub semantic_idle_ms: u64,
    /// Mechanical motion verdict (offline supplies the real kind; the live path
    /// passes `Still` and relies on `byte_idle_ms`).
    pub motion: Motion,
    /// Current VT cursor visibility.
    pub cursor_hidden: bool,
    /// Structural "input composer is ready" signal for full-screen TUI composers
    /// (e.g. opencode) whose idle prompt has no `❯`/`›`/`→` glyph: the VT cursor is
    /// visible and sits in the bottom input region of an alt-screen. Lets such
    /// providers settle to Idle promptly (structurally) instead of via the
    /// stuck-Working timeout. Computed by both adapters via [`composer_cursor_ready`].
    pub composer_ready: bool,
}

/// Structural input-composer readiness from VT cursor geometry (NOT prose): a
/// visible cursor parked in the bottom input region of the TUI composer. Covers
/// both alt-screen and primary-screen full-screen composers (e.g. opencode in
/// either mode) — a blocking picker/question hides the cursor or moves it into the
/// menu, so a *visible* cursor in the input region is an idle/ready signal. Shared
/// by the offline replay adapter and the live path so both agree.
pub fn composer_cursor_ready(cursor_visible: bool, cursor_row: u16, rows: u16) -> bool {
    cursor_visible && rows > 0 && cursor_row + BOTTOM_REGION_ROWS >= rows
}

/// The 3-state classification core — the ONE place the verdict is decided.
///
/// Order: picker question → structural working → rate-limit notice → idle
/// (quiet, settled, composer ready) → hold previous.
pub fn classify_state(input: &ClassifyInput, previous: TabVerdict) -> TabVerdict {
    // 1. Structured picker (API/SSE) -> question. The only card source; reliable.
    if let Some(question) = input.picker_question {
        return TabVerdict {
            state: TimelineState::Question,
            subtype: Some(classify_question_subtype(input.tail, question)),
        };
    }

    let ready_prompt_visible = ready_prompt_glyph(input.screen_lines);
    let live_status_widget = extract_status_line(input.screen_lines).is_some();
    let active_footer = active_turn_footer(input.screen_lines);
    let completed_status_repaint =
        completion_summary_immediately_precedes_status_repaint(input.screen_lines);
    // A still-animating turn (fresh bytes, no completed-turn summary) is genuinely live:
    // a long think repaints only the excluded status band, so semantic-idle keeps growing.
    let live_turn = (live_status_widget || active_footer)
        && input.byte_idle_ms < WORKING_GRACE_MS
        && extract_completion_summary(input.screen_lines).is_none()
        && !completed_status_repaint;
    let stale_working_status_at_prompt =
        ready_prompt_visible && input.semantic_idle_ms >= WORKING_GRACE_MS && !live_turn;

    // 2. Structurally active turn -> working. Do not treat raw PTY bytes as work:
    if !stale_working_status_at_prompt
        && (live_status_widget
            || active_footer
            || matches!(input.motion, Motion::Streaming | Motion::Animating))
    {
        return TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        };
    }

    // Prompt glyphs can false-fire on a single repaint frame.
    if previous.state == TimelineState::Working && input.semantic_idle_ms < WORKING_GRACE_MS {
        return TabVerdict {
            state: TimelineState::Working,
            subtype: None,
        };
    }

    if mode_switch_idle_footer(input.screen_lines) && ready_prompt_glyph_present(input.screen_lines)
    {
        return TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    if let Some(subtype) = classify_rate_limit_notice(input.screen_lines) {
        return TabVerdict {
            state: TimelineState::Question,
            subtype: Some(subtype),
        };
    }

    if ready_prompt_visible {
        return TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    // (full-screen OR primary-screen TUI composer with no glyph, e.g. opencode).
    if input.composer_ready {
        return TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    if previous.state == TimelineState::Working
        && input.byte_idle_ms >= STUCK_WORKING_IDLE_MS
        && input.semantic_idle_ms >= STUCK_WORKING_IDLE_MS
    {
        return TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    previous
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundarySignals {
    pub state: TimelineState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<QuestionSubtype>,
    pub byte_idle_ms: u64,
    pub semantic_idle_ms: u64,
    pub picker_active: bool,
    pub picker_detected: bool,
    pub rate_limit_notice_detected: bool,
    pub ready_prompt: bool,
}

pub fn classify_timeline(cast: &CastTimelineInput) -> Vec<ClassifiedSegment> {
    let builder = ClassifyInputBuilder::for_rows(cast.status_bar_rows);
    let mut extractor = SignalExtractor::new(cast.height, cast.width, builder.volatile_hint());
    let mut samples = Vec::new();
    let mut previous = TabVerdict {
        state: TimelineState::Idle,
        subtype: None,
    };
    let mut last_ms = 0u64;

    for event in &cast.events {
        if !samples.is_empty() {
            let mut tick = last_ms + TICK_MS;
            while tick < event.now_ms() {
                let signals = extractor.step(b"", tick);
                previous = classify_signals(&extractor, &signals, previous, &builder);
                samples.push((tick, previous));
                tick += TICK_MS;
            }
        }

        match event {
            CastEvent::Resize { now_ms, rows, cols } => {
                extractor.resize(*rows, *cols);
                last_ms = *now_ms;
            }
            CastEvent::Output { now_ms, bytes } => {
                let signals = extractor.step(bytes, *now_ms);
                previous = classify_signals(&extractor, &signals, previous, &builder);
                samples.push((*now_ms, previous));
                last_ms = *now_ms;
            }
        }
    }

    let end_ms = last_ms + DONE_MS + TICK_MS;
    let mut tick = last_ms + TICK_MS;
    while tick <= end_ms {
        let signals = extractor.step(b"", tick);
        previous = classify_signals(&extractor, &signals, previous, &builder);
        samples.push((tick, previous));
        tick += TICK_MS;
    }

    collapse_samples(samples, end_ms)
}

pub fn render_signals_at(cast: &CastTimelineInput, at_ms: u64) -> BoundarySignals {
    let builder = ClassifyInputBuilder::for_rows(cast.status_bar_rows);
    let mut extractor = SignalExtractor::new(cast.height, cast.width, builder.volatile_hint());

    for event in &cast.events {
        if event.now_ms() > at_ms {
            break;
        }
        match event {
            CastEvent::Resize { rows, cols, .. } => {
                extractor.resize(*rows, *cols);
            }
            CastEvent::Output { now_ms, bytes } => {
                extractor.step(bytes, *now_ms);
            }
        }
    }

    let signals = extractor.step(b"", at_ms);
    let picker = detect_picker(&extractor.attributed_screen());
    let rate_limit_notice = classify_rate_limit_notice(&signals.screen.lines);
    let verdict = classify_signals(
        &extractor,
        &signals,
        TabVerdict {
            state: TimelineState::Idle,
            subtype: None,
        },
        &builder,
    );
    BoundarySignals {
        state: verdict.state,
        subtype: verdict.subtype,
        byte_idle_ms: signals.quiescence.byte_idle_ms,
        semantic_idle_ms: signals.quiescence.semantic_idle_ms,
        picker_active: signals.picker_active,
        picker_detected: picker.is_some(),
        rate_limit_notice_detected: rate_limit_notice.is_some(),
        ready_prompt: ready_prompt(&signals.screen.lines),
    }
}

/// Offline adapter: build a [`ClassifyInput`] from the replayed `SignalExtractor`
/// (running `detect_picker` over its grid) and delegate to [`classify_state`].
/// Public so the replay renderer (`examples/render_timeline.rs`) drives the SAME
/// stateful `classify_state` semantics as the live path — one oracle, not two.
pub fn classify_signals(
    extractor: &SignalExtractor,
    signals: &StreamSignals,
    previous: TabVerdict,
    builder: &ClassifyInputBuilder,
) -> TabVerdict {
    let attributed = extractor.attributed_screen();
    let active_working_chrome = extract_status_line(&signals.screen.lines).is_some()
        || active_turn_footer(&signals.screen.lines);
    let within_working_grace = previous.state == TimelineState::Working
        && signals.quiescence.semantic_idle_ms < WORKING_GRACE_MS;
    let picker = (signals.picker_active && !active_working_chrome && !within_working_grace)
        .then(|| detect_picker(&attributed))
        .flatten()
        .filter(|picker| offline_picker_is_coherent(picker, signals));
    classify_state(
        &builder.build(ClassifyParts {
            picker_question: picker.as_ref().map(|p| p.question.as_str()),
            screen_lines: &signals.screen.lines,
            tail: &signals.tail,
            byte_idle_ms: signals.quiescence.byte_idle_ms,
            semantic_idle_ms: signals.quiescence.semantic_idle_ms,
            motion: Motion::Still,
            cursor_hidden: signals.input_mode.cursor_hidden,
            composer_ready: composer_cursor_ready(
                signals.cursor.visible,
                signals.cursor.row,
                signals.rows,
            ),
        }),
        previous,
    )
}

fn offline_picker_is_coherent(picker: &PickerDetection, signals: &StreamSignals) -> bool {
    let composer_row = ready_prompt_glyph_row(&signals.screen.lines).or_else(|| {
        composer_cursor_ready(signals.cursor.visible, signals.cursor.row, signals.rows)
            .then_some(signals.cursor.row)
    });
    if let Some(row) = composer_row {
        return picker.bottom_row + 1 >= row;
    }
    true
}

fn classify_question_subtype(_tail: &str, question: &str) -> QuestionSubtype {
    let question_text = question.to_lowercase();
    // picker's tail often carries unrelated output ("failed", "error", "permissions:")
    if contains_any(
        &question_text,
        &[
            "model",
            "available models",
            "select model",
            "reasoning level",
            "effort",
        ],
    ) {
        QuestionSubtype::ModelPicker
    } else if contains_any(
        &question_text,
        &[
            "connect a provider",
            "provider",
            "trust",
            "sign in",
            "log in",
            "login",
            "authenticate",
            "api key",
            "oauth",
        ],
    ) {
        QuestionSubtype::Auth
    } else if contains_any(
        &question_text,
        &[
            "permission",
            "approve",
            "allow",
            "deny",
            "run command",
            "run the following",
            "execute",
        ],
    ) {
        QuestionSubtype::Permission
    } else if contains_any(&question_text, &["choose", "select", "which", "pick"]) {
        QuestionSubtype::ModelChoice
    } else {
        QuestionSubtype::Unknown
    }
}

fn classify_rate_limit_notice(screen_lines: &[String]) -> Option<QuestionSubtype> {
    let text = screen_lines.join("\n").to_lowercase();
    if contains_any(
        &text,
        &[
            "rate limit reached",
            "usage limit reached",
            "reached your usage limit",
            "too many requests",
            "429 too many requests",
        ],
    ) {
        Some(QuestionSubtype::RateLimit)
    } else {
        None
    }
}

pub fn extract_question_message(
    screen_lines: &[String],
    subtype: Option<QuestionSubtype>,
) -> Option<String> {
    let phrases: &[&str] = match subtype {
        Some(QuestionSubtype::RateLimit) => &[
            "rate limit reached",
            "usage limit reached",
            "reached your usage limit",
            "too many requests",
            "429 too many requests",
        ],
        Some(QuestionSubtype::Auth) => &[
            "please log in",
            "please sign in",
            "you are not logged in",
            "not signed in",
            "authentication required",
            "log in to continue",
            "sign in to continue",
        ],
        _ => return None,
    };
    let index = screen_lines.iter().rposition(|line| {
        let lower = line.to_lowercase();
        phrases.iter().any(|phrase| lower.contains(phrase))
    })?;
    let mut start = index;
    while start > 0 && !screen_lines[start - 1].trim().is_empty() {
        start -= 1;
    }
    let mut end = index + 1;
    while end < screen_lines.len() && !screen_lines[end].trim().is_empty() {
        end += 1;
    }
    let message = screen_lines[start..end]
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!message.is_empty()).then_some(message)
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn ready_prompt(screen_lines: &[String]) -> bool {
    if active_turn_footer(screen_lines) || extract_status_line(screen_lines).is_some() {
        return false;
    }
    ready_prompt_glyph(screen_lines)
}

fn ready_prompt_glyph(screen_lines: &[String]) -> bool {
    ready_prompt_glyph_row(screen_lines).is_some()
}

/// Claude's idle ready-prompt footer: the mode-cycle line (`⏵⏵ … shift+tab to
/// cycle`). Shown ONLY at the idle composer — an active picker/permission overlay
/// replaces the bottom region and hides it. Scanned over the recent tail so
/// trailing stale picker paint (an answered question echo) cannot evict it.
fn mode_switch_idle_footer(screen_lines: &[String]) -> bool {
    screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .rev()
        .take(12)
        .any(|line| {
            let lower = line.to_lowercase();
            lower.contains("shift+tab") && lower.contains("cycle")
        })
}

/// A composer prompt glyph (❯/›/→) anywhere in the recent tail. Wider than
/// [`ready_prompt_glyph_row`] (last 4 non-empty) so stale picker paint below the
/// composer does not hide the real ready prompt.
fn ready_prompt_glyph_present(screen_lines: &[String]) -> bool {
    screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .rev()
        .take(12)
        .any(|line| line.starts_with('❯') || line.starts_with('›') || line.starts_with('→'))
}

fn ready_prompt_glyph_row(screen_lines: &[String]) -> Option<u16> {
    screen_lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(4)
        .find_map(|(idx, line)| {
            let line = line.trim();
            // Input-prompt glyphs: ❯/› (claude/codex), → (cursor's composer arrow).
            (line.starts_with('❯') || line.starts_with('›') || line.starts_with('→'))
                .then_some(idx as u16)
        })
}

fn has_progress_meter_shape(row: &str) -> bool {
    row.chars()
        .take(24)
        .filter(|c| matches!(c, '■' | '⬝' | '▰' | '▱' | '─' | '━' | '·'))
        .count()
        >= 3
}

pub(super) fn active_turn_footer(screen_lines: &[String]) -> bool {
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    nonempty.iter().rev().take(8).any(|line| {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        let has_interrupt_hint =
            lower.contains("esc to interrupt") || lower.contains("esc interrupt");
        let is_mode_switch_footer = lower.contains("shift+tab") && lower.contains("cycle");
        let has_stop_hint = lower.contains("ctrl+c to stop");
        (trimmed.starts_with("⏵⏵") && has_interrupt_hint && !is_mode_switch_footer)
            || (has_interrupt_hint && has_progress_meter_shape(trimmed))
            || (has_interrupt_hint && parse_elapsed_ms(trimmed).is_some())
            || (trimmed.starts_with('→') && has_stop_hint)
    })
}

/// Sum every `<digits><h|m|s>` token inside the first `(...)` group → ms.
/// Handles `(2s`, `(12s`, `(3m 54s`, `(1h 02m 10s`; ignores `↓ 17.0k tokens`.
pub(super) fn parse_elapsed_ms(row: &str) -> Option<u64> {
    let open = row.find('(')?;
    let rest = &row[open + 1..];
    let end = rest.find(')').unwrap_or(rest.len());
    let inner = &rest[..end];
    let b = inner.as_bytes();
    let mut i = 0;
    let mut total = 0u64;
    let mut found = false;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let s = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let mult = match b.get(i) {
                Some(b'h') => 3_600_000,
                Some(b'm') => 60_000,
                Some(b's') => 1_000,
                _ => 0,
            };
            if mult > 0 {
                if let Ok(n) = inner[s..i].parse::<u64>() {
                    total += n * mult;
                    found = true;
                }
                i += 1;
                continue;
            }
        }
        i += 1;
    }
    if found {
        Some(total)
    } else {
        None
    }
}

fn collapse_samples(samples: Vec<(u64, TabVerdict)>, end_ms: u64) -> Vec<ClassifiedSegment> {
    let Some((first_ms, first)) = samples.first().copied() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut from_ms = first_ms;
    let mut current = first;

    for (now_ms, verdict) in samples.into_iter().skip(1) {
        if verdict != current {
            out.push(ClassifiedSegment {
                from_ms,
                to_ms: now_ms,
                state: current.state,
                subtype: current.subtype,
            });
            from_ms = now_ms;
            current = verdict;
        }
    }

    out.push(ClassifiedSegment {
        from_ms,
        to_ms: end_ms.max(from_ms),
        state: current.state,
        subtype: current.subtype,
    });
    out
}

#[cfg(test)]
mod tests;
