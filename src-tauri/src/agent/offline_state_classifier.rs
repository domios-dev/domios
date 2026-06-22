use serde::{Deserialize, Serialize};

use super::attributed_screen::{detect_picker, PickerDetection};
use super::signal_extractor::{Motion, SignalExtractor, StreamSignals, VolatileHint};

mod status;

pub use status::{
    extract_completion_summary, extract_status_line, has_active_turn_footer, StatusLine,
};

pub const TICK_MS: u64 = 250;
pub const WAIT_MS: u64 = 800;
pub const DONE_MS: u64 = 800;
const WORKING_GRACE_MS: u64 = 1500;
const BOTTOM_REGION_ROWS: u16 = 8;
/// Upper bound on how long a Working verdict may persist with zero active-work
/// evidence and total PTY silence before resolving to Idle (anti-stuck-Working).
const STUCK_WORKING_IDLE_MS: u64 = 5000;

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

/// Veto for the idle fast-paths: the BOTTOM row is a horizontal option/button
/// block (a possibly-uncaught PTY question), so a glyph/visible-cursor must NOT be
/// read as an idle composer and mask it. Scope is deliberately narrow:
/// - VERTICAL numbered/checkbox menus are NOT vetoed here — an ACTIVE one is already
///   classified Question by `classify_terminal_menu_question` (runs earlier), and an
///   ANSWERED one sits in scrollback ABOVE the composer (cursor's `[x] A / [ ] B`
///   poll), which must still resolve Idle. Vetoing those would re-strand cursor.
/// - HORIZONTAL buttons (a `❯/›` marker row laid out in COLUMNS, ≥2 multi-space
///   gaps like `❯ Yes      No      Always`, or a row with ≥2 bracketed tokens) ARE
///   vetoed: they are not caught elsewhere and are a real selection surface. The
///   column-gap test separates them from a composer holding free text
///   (`› еще раз а ты можешь…`, single-spaced), which is never vetoed.
fn looks_like_options_block(screen_lines: &[String]) -> bool {
    let Some(bottom) = screen_lines
        .iter()
        .map(|r| r.trim())
        .filter(|r| !r.is_empty())
        .next_back()
    else {
        return false;
    };
    let starts_marker = bottom.starts_with('❯') || bottom.starts_with('›');
    (starts_marker && multi_space_gaps(bottom) >= 2) || bottom.matches('[').count() >= 2
}

/// Count maximal runs of ≥2 spaces — column separators in a TUI button/menu row.
fn multi_space_gaps(s: &str) -> usize {
    let mut gaps = 0usize;
    let mut run = 0usize;
    for c in s.chars() {
        if c == ' ' {
            run += 1;
        } else {
            if run >= 2 {
                gaps += 1;
            }
            run = 0;
        }
    }
    if run >= 2 {
        gaps += 1;
    }
    gaps
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

    let options_block = looks_like_options_block(input.screen_lines);
    let ready_prompt_visible = !options_block && ready_prompt_glyph(input.screen_lines);
    let stale_working_status_at_prompt =
        ready_prompt_visible && input.semantic_idle_ms >= WORKING_GRACE_MS;

    // 2. Structurally active turn -> working. Do not treat raw PTY bytes as work:
    // composer redraws and bracketed-paste echoes are PTY output too.
    // NOTE: the `motion` arm is currently DEAD — both callers (live `recompute_
    // agent_state` and offline `classify_signals`) pass `Motion::Still` so replay
    // and live agree (live cannot compute motion). Retained, explicitly gated, for
    // a future motion-aware caller; today working is decided purely by the status
    // widget / active-turn footer, which covers the corpus (verified byte-identical).
    if !stale_working_status_at_prompt
        && (extract_status_line(input.screen_lines).is_some()
            || active_turn_footer(input.screen_lines)
            || matches!(input.motion, Motion::Streaming | Motion::Animating))
    {
        return PaneVerdict {
            state: TimelineState::Working,
            subtype: None,
        };
    }

    // Working-grace BEFORE PTY-only questions and idle fast-paths: both terminal
    // menu parsing and prompt glyphs can false-fire on a single repaint frame.
    // Structured prompts already returned above and remain immediate.
    if previous.state == TimelineState::Working && input.semantic_idle_ms < WORKING_GRACE_MS {
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

    // A horizontal/vertical options block at the bottom (e.g. `❯ Yes  No  Always`)
    // is a (possibly uncaught) question surface, NOT a composer — it must not be
    // read as Idle by either the glyph fast-path or composer_ready below. Computed
    // once; vetoes both idle paths so a real PTY question is never masked as Idle.
    if ready_prompt_visible {
        return PaneVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    // Weaker structural idle: a visible cursor parked in the bottom input region
    // (full-screen OR primary-screen TUI composer with no glyph, e.g. opencode).
    // Gated AFTER grace and vetoed when the bottom looks like an (uncaught) options
    // block, so it can neither pre-empt working-grace nor mask a real PTY question.
    if input.composer_ready && !options_block {
        return PaneVerdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    // Safety net against "stuck Working": reaching here means none of the active
    // signals fired (no status widget, no active-turn footer, no motion, no
    // picker/question, no recognized ready-prompt). A Working pane that has gone
    // fully silent on BOTH clocks past the stuck threshold is not working — never
    // inherit Working indefinitely just because the composer glyph was not matched.
    // Requiring byte-silence too means an animating spinner (volatile, byte_idle≈0)
    // never trips this.
    if previous.state == TimelineState::Working
        && input.byte_idle_ms >= STUCK_WORKING_IDLE_MS
        && input.semantic_idle_ms >= STUCK_WORKING_IDLE_MS
    {
        return PaneVerdict {
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
/// Public so the replay renderer (`examples/render_timeline.rs`) drives the SAME
/// stateful `classify_state` semantics as the live path — one oracle, not two.
pub fn classify_signals(
    extractor: &SignalExtractor,
    signals: &StreamSignals,
    previous: PaneVerdict,
) -> PaneVerdict {
    // Fidelity gate: live `recompute_agent_state` takes its picker question ONLY
    // from a STRUCTURED handle (output.rs hard-restriction) — never from native
    // grid parsing. Offline has no SSE stream, so `detect_picker` is the stand-in,
    // but it must fire ONLY while the picker is structurally ACTIVE (`picker_active`
    // is a narrow hidden-cursor/still-screen signal, not alt-screen alone).
    // Otherwise a picker's options
    // left rendered in the transcript after it was answered (e.g. cursor's inline
    // "AskQuestion" poll) keep matching the grid and strand the pane in Question.
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
        &ClassifyInput {
            picker_question: picker.as_ref().map(|p| p.question.as_str()),
            screen_lines: &signals.screen.lines,
            tail: &signals.tail,
            byte_idle_ms: signals.quiescence.byte_idle_ms,
            semantic_idle_ms: signals.quiescence.semantic_idle_ms,
            // Parity with the live path, which cannot compute motion and always
            // passes `Still`: feeding the replay's real motion here would let the
            // oracle reach Working via a signal the live classifier never sees.
            // Verified redundant — status_line / active_turn_footer / composer_ready
            // cover every working frame in the corpus, so forcing `Still` leaves all
            // replay verdicts byte-identical while keeping replay==live.
            motion: Motion::Still,
            cursor_hidden: signals.input_mode.cursor_hidden,
            composer_ready: composer_cursor_ready(
                signals.cursor.visible,
                signals.cursor.row,
                signals.rows,
            ),
        },
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

pub fn classify_terminal_menu_question(
    screen_lines: &[String],
    cursor_hidden: bool,
) -> Option<QuestionSubtype> {
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
    // A confirm-style key-hint footer (enter + esc + a nav/choose hint) is a strong
    // active-choice signal on its own, so options + that footer classify Question
    // EVEN WITH A VISIBLE CURSOR — otherwise a permission/choice box with a visible
    // cursor would be false-idled by composer_ready (which runs later). Without that
    // footer we still require the hidden-cursor interactive-mode signal to avoid
    // turning plain numbered prose into a question.
    let confirm_footer = confirm_style_footer(screen_lines);
    let has_options = numbered >= 2 || checkbox >= 2;
    if has_options && confirm_footer {
        return Some(QuestionSubtype::Unknown);
    }
    if !cursor_hidden {
        return None;
    }
    let has_choice_footer = choice_key_footer(screen_lines);
    ((selected == 1 && numbered >= 2)
        || (numbered >= 2 && has_choice_footer)
        || (checkbox >= 2 && has_choice_footer))
        .then_some(QuestionSubtype::Unknown)
}

/// A confirm/choose key-hint footer: the same line tells the user how to navigate
/// (arrows / "up/down" / space), confirm (enter), and cancel (esc). Broader than
/// [`choice_key_footer`] (which demands the exact "submit"/"dismiss" tokens) so it
/// also matches confirm-style permission prompts (`… Enter to confirm, Esc to
/// cancel`). Key-hint structure, paired with on-screen options — not free prose.
fn confirm_style_footer(screen_lines: &[String]) -> bool {
    screen_lines.iter().rev().take(8).any(|line| {
        let lower = line.to_lowercase();
        let has_nav = line.contains('↑')
            || line.contains('↓')
            || lower.contains("up/down")
            || lower.contains("up / down")
            || lower.contains("arrow")
            || lower.contains("space")
            || lower.contains("choose")
            || lower.contains("select");
        let has_confirm = lower.contains("enter") || lower.contains("return");
        let has_cancel = lower.contains("esc"); // esc / escape
        has_nav && has_confirm && has_cancel
    })
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
    ready_prompt_glyph(screen_lines)
}

fn ready_prompt_glyph(screen_lines: &[String]) -> bool {
    ready_prompt_glyph_row(screen_lines).is_some()
}

fn ready_prompt_glyph_row(screen_lines: &[String]) -> Option<u16> {
    // Last 4 NON-EMPTY rows: a full-height grid leaves blank padding rows below
    // the composer, so counting raw rows would push the prompt glyph out of the
    // window and strand the pane in its previous (often Working) verdict.
    screen_lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, line)| !line.trim().is_empty())
        .take(4)
        .find_map(|(idx, line)| {
            let line = line.trim();
            // Input-prompt glyphs: ❯/› (claude/codex), → (cursor's composer arrow).
            // Structural input indicators, not prose. ready_prompt only runs when
            // not actively working (guarded above), so cursor's working footer
            // `→ … ctrl+c to stop` is caught by active_turn_footer first.
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
mod tests;
