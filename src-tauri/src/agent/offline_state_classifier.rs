use serde::{Deserialize, Serialize};

use super::attributed_screen::detect_picker;
use super::signal_extractor::{Motion, SignalExtractor, StreamSignals, VolatileHint};

pub const TICK_MS: u64 = 250;
pub const WAIT_MS: u64 = 800;
pub const DONE_MS: u64 = 800;
const WORKING_GRACE_MS: u64 = 1500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineState {
    Working,
    Idle,
    Question,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionSubtype {
    ModelChoice,
    ModelPicker,
    Permission,
    Auth,
    RateLimit,
    Update,
    Error,
    Unknown,
}

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
    pub events: Vec<CastEvent>,
}

/// The per-tick classification result: the 3-state verdict plus, when in
/// `Question`, its subtype. Produced by [`classify_state`] — the single source
/// of truth shared by the offline cast replay and the live PTY path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneVerdict {
    pub state: TimelineState,
    pub subtype: Option<QuestionSubtype>,
}

/// Persisted-to-frontend snapshot of a pane's live agent state. Stored on the
/// pane (runtime-only, recomputed from output) and serialized into the mux
/// snapshot so the chat view's state indicator renders it. Field names/casing
/// mirror the frontend `AgentState` shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLiveState {
    pub state: TimelineState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtype: Option<QuestionSubtype>,
    /// The agent's live status/progress line, surfaced verbatim by the chat UI
    /// while working (see [`extract_status_line`]). Only set in `Working`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    /// Completed-turn summary (e.g. "Churned for 1m 37s"), shown as a muted
    /// secondary line while idle. Set only in `Idle`; distinct from the active
    /// `status_line`. See [`extract_completion_summary`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_summary: Option<String>,
}

impl From<AgentLiveState> for PaneVerdict {
    fn from(s: AgentLiveState) -> Self {
        PaneVerdict {
            state: s.state,
            subtype: s.subtype,
        }
    }
}

impl From<PaneVerdict> for AgentLiveState {
    fn from(v: PaneVerdict) -> Self {
        AgentLiveState {
            state: v.state,
            subtype: v.subtype,
            status_line: None,
            phase: None,
            elapsed_ms: None,
            last_turn_summary: None,
        }
    }
}

/// Minimal per-tick inputs the 3-state engine classifies. Both paths build this
/// and call [`classify_state`]:
/// - offline replay fills `picker_question` by running `detect_picker` over the
///   replayed grid;
/// - the live PTY path passes the already-surfaced pending prompt's question so
///   it never re-parses the picker, and derives `screen_lines`/`tail` from the
///   existing per-pane `ScreenState` parser (no second vt100).
pub struct ClassifyInput<'a> {
    /// Pre-detected picker question, if a picker is on screen.
    pub picker_question: Option<&'a str>,
    /// Trimmed per-row screen text (text-question + ready-prompt fingerprints).
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
    /// Current VT cursor visibility. Used only for parked terminal menus; normal
    /// prose with numbered lists must not become a question.
    pub cursor_hidden: bool,
}

/// The 3-state classification core — the ONE place the verdict is decided.
///
/// Order: picker question → structural working → parked terminal-menu question
/// → text question → idle (quiet, settled, composer ready) → hold previous.
pub fn classify_state(input: &ClassifyInput, previous: PaneVerdict) -> PaneVerdict {
    // 1. Structured picker (API/SSE) -> question. The only card source; reliable.
    if let Some(question) = input.picker_question {
        return PaneVerdict {
            state: TimelineState::Question,
            subtype: Some(classify_question_subtype(input.tail, question)),
        };
    }

    // 2. Structurally active turn -> working. Do not treat raw PTY bytes as work:
    // composer redraws and bracketed-paste echoes are PTY output too.
    if extract_status_line(input.screen_lines).is_some()
        || active_turn_footer(input.screen_lines)
        || matches!(input.motion, Motion::Streaming | Motion::Animating)
    {
        return PaneVerdict {
            state: TimelineState::Working,
            subtype: None,
        };
    }

    if let Some(subtype) = classify_terminal_menu_question(input.screen_lines, input.cursor_hidden)
    {
        return PaneVerdict {
            state: TimelineState::Question,
            subtype: Some(subtype),
        };
    }

    // 3. QUIESCENT only: the two known console question states (rate_limit / auth),
    //    strict phrasings. Nothing generic is inferred from raw console text.
    if let Some(subtype) = classify_text_question(input.screen_lines) {
        return PaneVerdict {
            state: TimelineState::Question,
            subtype: Some(subtype),
        };
    }

    if ready_prompt(input.screen_lines) {
        return PaneVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    if previous.state == TimelineState::Working && input.semantic_idle_ms < WORKING_GRACE_MS {
        return PaneVerdict {
            state: TimelineState::Working,
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
    pub text_question_detected: bool,
    pub ready_prompt: bool,
}

pub fn classify_timeline(cast: &CastTimelineInput) -> Vec<ClassifiedSegment> {
    let mut extractor = SignalExtractor::new(cast.height, cast.width, VolatileHint::default());
    let mut samples = Vec::new();
    let mut previous = PaneVerdict {
        state: TimelineState::Idle,
        subtype: None,
    };
    let mut last_ms = 0u64;

    for event in &cast.events {
        if !samples.is_empty() {
            let mut tick = last_ms + TICK_MS;
            while tick < event.now_ms() {
                let signals = extractor.step(b"", tick);
                previous = classify_signals(&extractor, &signals, previous);
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
                previous = classify_signals(&extractor, &signals, previous);
                samples.push((*now_ms, previous));
                last_ms = *now_ms;
            }
        }
    }

    let end_ms = last_ms + DONE_MS + TICK_MS;
    let mut tick = last_ms + TICK_MS;
    while tick <= end_ms {
        let signals = extractor.step(b"", tick);
        previous = classify_signals(&extractor, &signals, previous);
        samples.push((tick, previous));
        tick += TICK_MS;
    }

    collapse_samples(samples, end_ms)
}

pub fn render_signals_at(cast: &CastTimelineInput, at_ms: u64) -> BoundarySignals {
    let mut extractor = SignalExtractor::new(cast.height, cast.width, VolatileHint::default());

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
    let text_question = classify_text_question(&signals.screen.lines);
    let verdict = classify_signals(
        &extractor,
        &signals,
        PaneVerdict {
            state: TimelineState::Idle,
            subtype: None,
        },
    );
    BoundarySignals {
        state: verdict.state,
        subtype: verdict.subtype,
        byte_idle_ms: signals.quiescence.byte_idle_ms,
        semantic_idle_ms: signals.quiescence.semantic_idle_ms,
        picker_active: signals.picker_active,
        picker_detected: picker.is_some(),
        text_question_detected: text_question.is_some(),
        ready_prompt: ready_prompt(&signals.screen.lines),
    }
}

/// Offline adapter: build a [`ClassifyInput`] from the replayed `SignalExtractor`
/// (running `detect_picker` over its grid) and delegate to [`classify_state`].
fn classify_signals(
    extractor: &SignalExtractor,
    signals: &StreamSignals,
    previous: PaneVerdict,
) -> PaneVerdict {
    let attributed = extractor.attributed_screen();
    let picker = detect_picker(&attributed);
    classify_state(
        &ClassifyInput {
            picker_question: picker.as_ref().map(|p| p.question.as_str()),
            screen_lines: &signals.screen.lines,
            tail: &signals.tail,
            byte_idle_ms: signals.quiescence.byte_idle_ms,
            semantic_idle_ms: signals.quiescence.semantic_idle_ms,
            motion: signals.quiescence.kind,
            cursor_hidden: signals.input_mode.cursor_hidden,
        },
        previous,
    )
}

pub fn classify_terminal_menu_question(
    screen_lines: &[String],
    cursor_hidden: bool,
) -> Option<QuestionSubtype> {
    if !cursor_hidden {
        return None;
    }
    let mut selected = 0usize;
    let mut numbered = 0usize;
    let mut checkbox = 0usize;
    for line in screen_lines {
        let (marked, unmarked) = terminal_menu_payload(line);
        if numbered_menu_line(unmarked) {
            if marked {
                selected += 1;
            }
            numbered += 1;
        }
        if checkbox_menu_line(unmarked) {
            checkbox += 1;
        }
    }
    let has_choice_footer = choice_key_footer(screen_lines);
    ((selected == 1 && numbered >= 2)
        || (numbered >= 2 && has_choice_footer)
        || (checkbox >= 2 && has_choice_footer))
        .then_some(QuestionSubtype::Unknown)
}

fn terminal_menu_payload(line: &str) -> (bool, &str) {
    let mut trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix('│')
        .or_else(|| trimmed.strip_prefix('┃'))
    {
        trimmed = rest.trim_start();
    }
    let marked = trimmed.starts_with('›') || trimmed.starts_with('❯');
    if let Some(rest) = trimmed
        .strip_prefix('›')
        .or_else(|| trimmed.strip_prefix('❯'))
    {
        trimmed = rest.trim_start();
    }
    (marked, trimmed)
}

fn numbered_menu_line(line: &str) -> bool {
    let line = line.trim_start();
    let mut chars = line.chars().peekable();
    let mut digits = 0usize;
    while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
        digits += 1;
        chars.next();
    }
    digits > 0 && matches!(chars.next(), Some('.'))
}

fn checkbox_menu_line(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("[ ]")
        || line.starts_with("[x]")
        || line.starts_with("[X]")
        || line.starts_with("( )")
        || line.starts_with("(x)")
        || line.starts_with("(X)")
}

fn choice_key_footer(screen_lines: &[String]) -> bool {
    screen_lines.iter().rev().take(8).any(|line| {
        let line = line.trim();
        let lower = line.to_lowercase();
        let has_navigation = line.contains('↑')
            || line.contains('↓')
            || line.contains('←')
            || line.contains('→')
            || lower.contains("space");
        let has_submit = lower.contains("enter") && lower.contains("submit");
        let has_escape =
            lower.contains("esc") && (lower.contains("dismiss") || lower.contains("skip"));
        has_navigation && has_submit && has_escape
    })
}

fn classify_question_subtype(_tail: &str, question: &str) -> QuestionSubtype {
    let question_text = question.to_lowercase();
    // Subtype is keyed ONLY off the picker's QUESTION (never the tail): a structured
    // picker's tail often carries unrelated output ("failed", "error", "permissions:")
    // that would mislabel it. A picker is one of model_choice/model_picker/permission/
    // auth — never rate_limit/update/error (those are text-question states, not pickers).
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

fn classify_text_question(screen_lines: &[String]) -> Option<QuestionSubtype> {
    // HARD RESTRICTION: only the two console question states we are confident about,
    // with STRICT notice/prompt phrasings that do not occur in ordinary agent output.
    // Everything else (model / permission / error / update / generic) is NEVER inferred
    // from raw console text — those come only from structured handles (API/SSE), or not
    // at all. This is gated by the caller to run ONLY when the pane is quiescent.
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
    } else if contains_any(
        &text,
        &[
            "please log in",
            "please sign in",
            "you are not logged in",
            "not signed in",
            "authentication required",
            "log in to continue",
            "sign in to continue",
        ],
    ) {
        Some(QuestionSubtype::Auth)
    } else {
        None
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn ready_prompt(screen_lines: &[String]) -> bool {
    if active_turn_footer(screen_lines) || extract_status_line(screen_lines).is_some() {
        return false;
    }
    screen_lines.iter().rev().take(4).any(|line| {
        let line = line.trim();
        line.starts_with('❯') || line.starts_with('›')
    })
}

fn active_turn_footer(screen_lines: &[String]) -> bool {
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
        let has_stop_hint = lower.contains("ctrl+c to stop");
        (trimmed.starts_with("⏵⏵") && has_interrupt_hint)
            || (has_interrupt_hint && has_progress_meter_shape(trimmed))
            || (has_interrupt_hint && parse_elapsed_ms(trimmed).is_some())
            || (trimmed.starts_with('→') && has_stop_hint)
    })
}

/// A live status/progress widget extracted from the rendered screen — the line
/// the UI surfaces verbatim while the agent works (e.g. claude's
/// `Swooping… (6m 24s · ↓ 29.8k tokens · still thinking with high effort)`).
/// Provider-tuned today; generalized via the replay loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub line: String,
    pub phase: Option<String>,
    pub elapsed_ms: Option<u64>,
}

const STATUS_SPINNERS: &[char] = &['✻', '✳', '✢', '✶', '✷', '✺', '✽', '●', '∗', '⋆'];
const STATUS_MESSAGE_MAX_CHARS: usize = 80;

fn compact_status_message(line: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in line.chars().enumerate() {
        if idx >= STATUS_MESSAGE_MAX_CHARS {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

fn is_braille_spinner(c: char) -> bool {
    ('\u{2800}'..='\u{28ff}').contains(&c)
}

fn has_progress_meter_shape(row: &str) -> bool {
    row.chars()
        .take(24)
        .filter(|c| matches!(c, '■' | '⬝' | '▰' | '▱' | '─' | '━' | '·'))
        .count()
        >= 3
}

/// Sum every `<digits><h|m|s>` token inside the first `(...)` group → ms.
/// Handles `(2s`, `(12s`, `(3m 54s`, `(1h 02m 10s`; ignores `↓ 17.0k tokens`.
fn parse_elapsed_ms(row: &str) -> Option<u64> {
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

/// Scan the bottom band of the rendered screen for the live status widget. A row
/// qualifies ONLY with the structural signature — an ellipsis plus a timer or
/// token meter, or a strong spinner plus a timer — so ordinary prose and the
/// persistent mode line (`⏵⏵ auto mode … esc to interrupt`, which has neither)
/// never read as a status line. Validated on a 15-turn claude cast: 90% coverage
/// while working, zero false positives. The leading spinner glyph is stripped for
/// a clean UI line.
pub fn extract_status_line(screen_lines: &[String]) -> Option<StatusLine> {
    if let Some(status) = extract_opencode_status_line(screen_lines) {
        return Some(status);
    }
    if let Some(status) = extract_structural_status_line(screen_lines) {
        return Some(status);
    }
    // The last 16 NON-EMPTY rows: the widget can sit above blank input-box padding,
    // so counting blanks would shrink the real window.
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim_end())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(16);
    for row in nonempty[start..].iter().rev() {
        let row = *row;
        let has_ellipsis = row.contains('…') || row.contains("...");
        let elapsed_ms = parse_elapsed_ms(row);
        let has_token = row.to_lowercase().contains("tok");
        let strong_spinner = row.chars().any(|c| STATUS_SPINNERS.contains(&c));
        let is_widget = (has_ellipsis && (elapsed_ms.is_some() || has_token))
            || (strong_spinner && elapsed_ms.is_some());
        if !is_widget {
            continue;
        }
        let phase = status_phase_from_meter(row);
        let line = row.trim_start_matches(|c: char| {
            STATUS_SPINNERS.contains(&c) || c == '·' || c.is_whitespace()
        });
        return Some(StatusLine {
            line: compact_status_message(line),
            phase: Some(phase.to_string()),
            elapsed_ms,
        });
    }
    None
}

fn extract_structural_status_line(screen_lines: &[String]) -> Option<StatusLine> {
    if !active_turn_footer(screen_lines) {
        return None;
    }
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(14);
    for row in nonempty[start..].iter().rev() {
        if let Some(status) = extract_braille_status_row(row) {
            return Some(status);
        }
        if let Some(status) = extract_elapsed_interrupt_status_row(row) {
            return Some(status);
        }
    }
    None
}

fn extract_braille_status_row(row: &str) -> Option<StatusLine> {
    let trimmed = row.trim_start();
    let mut chars = trimmed.char_indices().peekable();
    let mut end = 0usize;
    let mut count = 0usize;
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            end = idx + ch.len_utf8();
            chars.next();
            continue;
        }
        if is_braille_spinner(ch) {
            end = idx + ch.len_utf8();
            count += 1;
            chars.next();
            continue;
        }
        break;
    }
    if count == 0 {
        return None;
    }
    let line = trimmed[end..].trim();
    if line.is_empty() || line.len() > 80 {
        return None;
    }
    Some(StatusLine {
        line: compact_status_message(line),
        phase: Some("work".to_string()),
        elapsed_ms: None,
    })
}

fn extract_elapsed_interrupt_status_row(row: &str) -> Option<StatusLine> {
    let trimmed = row.trim();
    let lower = trimmed.to_lowercase();
    if !(lower.contains("esc to interrupt") || lower.contains("esc interrupt")) {
        return None;
    }
    let elapsed_ms = parse_elapsed_ms(trimmed)?;
    let line = trimmed
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '•' | '*' | '-' | '·' | '●'))
        .to_string();
    if line.is_empty() || line.len() > 100 {
        return None;
    }
    Some(StatusLine {
        line: compact_status_message(&line),
        phase: Some("work".to_string()),
        elapsed_ms: Some(elapsed_ms),
    })
}

fn status_phase_from_meter(row: &str) -> &'static str {
    let _ = row;
    "work"
}

fn extract_opencode_status_line(screen_lines: &[String]) -> Option<StatusLine> {
    if !active_turn_footer(screen_lines) {
        return None;
    }
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(14);
    let mut fallback = None;
    for row in nonempty[start..].iter().rev() {
        let row = *row;
        if is_opencode_chrome_row(row) {
            continue;
        }
        let Some((phase, trimmed)) = strip_opencode_status_prefix(row) else {
            continue;
        };
        if trimmed.is_empty() || trimmed.len() > 80 {
            continue;
        }
        let status = StatusLine {
            line: compact_status_message(trimmed),
            phase: Some(phase.clone()),
            elapsed_ms: None,
        };
        if phase == "work" {
            fallback.get_or_insert(status);
            continue;
        }
        return Some(status);
    }
    fallback
}

fn is_opencode_chrome_row(row: &str) -> bool {
    let trimmed = row.trim_start();
    let lower = trimmed.to_lowercase();
    if lower.contains("esc to interrupt") || lower.contains("esc interrupt") {
        return true;
    }
    trimmed.starts_with('┃')
        || trimmed.starts_with('╹')
        || trimmed.starts_with('│')
        || trimmed
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '─' | '━' | '╹' | '╺' | '╸'))
}

fn strip_opencode_status_prefix(row: &str) -> Option<(String, &str)> {
    let trimmed = row.trim_start();
    let mut chars = trimmed.char_indices();
    let (_, first) = chars.next()?;
    let phase = if is_braille_spinner(first) {
        "thinking"
    } else if first == '~' {
        "streaming"
    } else if first == '▣' {
        "work"
    } else {
        return None;
    };
    let rest_start = first.len_utf8();
    Some((phase.to_string(), trimmed[rest_start..].trim()))
}

pub fn has_active_turn_footer(screen_lines: &[String]) -> bool {
    active_turn_footer(screen_lines)
}

/// Sum `<digits><h|m|s>` tokens ANYWHERE in `s` → ms. For the completed-turn
/// summary ("Churned for 1m 37s") the elapsed is not parenthesized.
fn sum_hms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut total = 0u64;
    let mut found = false;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let start = i;
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
                if let Ok(n) = s[start..i].parse::<u64>() {
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

/// The completed-turn summary claude shows once idle (e.g. `✻ Churned for 1m
/// 37s`). Structurally distinct from the ACTIVE widget: a strong spinner with a
/// bare h/m/s elapsed, but NO ellipsis and NO parenthesized timer. Surfaced as a
/// muted secondary line WHILE IDLE — never the working line, never keyed on the
/// verb (the gerund varies: Churned/Brewed/Sautéed/…).
pub fn extract_completion_summary(screen_lines: &[String]) -> Option<StatusLine> {
    let nonempty: Vec<&str> = screen_lines
        .iter()
        .map(|r| r.trim_end())
        .filter(|r| !r.is_empty())
        .collect();
    let start = nonempty.len().saturating_sub(16);
    for row in nonempty[start..].iter().rev() {
        let row = *row;
        // Skip the active widget (ellipsis / parenthesized timer) and chrome lines.
        if row.contains('…') || row.contains("...") || row.contains('(') {
            continue;
        }
        if !row.chars().any(|c| STATUS_SPINNERS.contains(&c)) {
            continue;
        }
        let Some(elapsed_ms) = sum_hms(row) else {
            continue;
        };
        let line = row.trim_start_matches(|c: char| {
            STATUS_SPINNERS.contains(&c) || c == '·' || c.is_whitespace()
        });
        return Some(StatusLine {
            line: compact_status_message(line),
            phase: Some("done".to_string()),
            elapsed_ms: Some(elapsed_ms),
        });
    }
    None
}

fn collapse_samples(samples: Vec<(u64, PaneVerdict)>, end_ms: u64) -> Vec<ClassifiedSegment> {
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
mod tests {
    use super::{classify_terminal_menu_question, QuestionSubtype};

    fn lines(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|row| (*row).to_string()).collect()
    }

    #[test]
    fn detects_boxed_checkbox_terminal_question() {
        let screen = lines(&[
            " │ CLI choice",
            " │ 1. Please choose one option:",
            " │   › [ ] Alpha",
            " │     [ ] Beta",
            " │     [ ] Gamma",
            " │ ↑/↓ option · ←/→ question · Space select · Enter next/submit · Esc to skip",
        ]);
        assert_eq!(
            classify_terminal_menu_question(&screen, true),
            Some(QuestionSubtype::Unknown)
        );
    }

    #[test]
    fn detects_boxed_numbered_terminal_question_without_selected_marker() {
        let screen = lines(&[
            "┃  Which option do you choose?",
            "┃",
            "┃  1. Alpha",
            "┃  2. Beta",
            "┃  3. Gamma",
            "┃",
            "┃  ↑↓ select  enter submit  esc dismiss",
        ]);
        assert_eq!(
            classify_terminal_menu_question(&screen, true),
            Some(QuestionSubtype::Unknown)
        );
    }

    #[test]
    fn ignores_plain_numbered_prose_without_terminal_choice_footer() {
        let screen = lines(&[
            "Here is a plain list:",
            "1. Alpha",
            "2. Beta",
            "3. Gamma",
            "No action is required.",
        ]);
        assert_eq!(classify_terminal_menu_question(&screen, true), None);
    }

    #[test]
    fn ignores_terminal_choice_shape_when_cursor_is_visible() {
        let screen = lines(&[
            "┃  1. Alpha",
            "┃  2. Beta",
            "┃  ↑↓ select  enter submit  esc dismiss",
        ]);
        assert_eq!(classify_terminal_menu_question(&screen, false), None);
    }
}
