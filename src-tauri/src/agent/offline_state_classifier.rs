use serde::{Deserialize, Serialize};

use super::attributed_screen::detect_picker;
use super::signal_extractor::{Motion, SignalExtractor, StreamSignals, VolatileHint};

pub const TICK_MS: u64 = 250;
pub const WAIT_MS: u64 = 800;
pub const DONE_MS: u64 = 800;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Verdict {
    state: TimelineState,
    subtype: Option<QuestionSubtype>,
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
    let mut previous = Verdict {
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
    let text_question = classify_text_question(&signals);
    let verdict = classify_signals(
        &extractor,
        &signals,
        Verdict {
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
        ready_prompt: ready_prompt(&signals),
    }
}

fn classify_signals(
    extractor: &SignalExtractor,
    signals: &StreamSignals,
    previous: Verdict,
) -> Verdict {
    if let Some(picker) = detect_picker(&extractor.attributed_screen()) {
        return Verdict {
            state: TimelineState::Question,
            subtype: Some(classify_question_subtype(signals, &picker.question)),
        };
    }

    if let Some(subtype) = classify_text_question(signals) {
        return Verdict {
            state: TimelineState::Question,
            subtype: Some(subtype),
        };
    }

    let q = signals.quiescence;
    if q.byte_idle_ms < WAIT_MS || matches!(q.kind, Motion::Streaming | Motion::Animating) {
        return Verdict {
            state: TimelineState::Working,
            subtype: None,
        };
    }

    if q.semantic_idle_ms >= DONE_MS && ready_prompt(signals) {
        return Verdict {
            state: TimelineState::Idle,
            subtype: None,
        };
    }

    previous
}

fn classify_question_subtype(signals: &StreamSignals, question: &str) -> QuestionSubtype {
    let question_text = question.to_lowercase();
    let text = format!("{question}\n{}", signals.tail).to_lowercase();
    if contains_any(
        &text,
        &["permission", "approve", "allow", "deny", "run command"],
    ) {
        QuestionSubtype::Permission
    } else if contains_any(
        &question_text,
        &[
            "model",
            "available models",
            "select model",
            "reasoning level",
        ],
    ) {
        QuestionSubtype::ModelPicker
    } else if contains_any(&question_text, &["connect a provider", "provider"])
        || contains_any(
            &text,
            &["login", "sign in", "oauth", "api key", "authenticate"],
        )
    {
        QuestionSubtype::Auth
    } else if contains_any(
        &text,
        &["rate limit", "usage limit", "quota", "try again later"],
    ) {
        QuestionSubtype::RateLimit
    } else if contains_any(&text, &["update", "new version", "upgrade"]) {
        QuestionSubtype::Update
    } else if contains_any(&text, &["error", "failed", "crash"]) {
        QuestionSubtype::Error
    } else if contains_any(&text, &["model", "provider", "choose", "select"]) {
        QuestionSubtype::ModelChoice
    } else {
        QuestionSubtype::Unknown
    }
}

fn classify_text_question(signals: &StreamSignals) -> Option<QuestionSubtype> {
    let text = signals.screen.lines.join("\n").to_lowercase();
    if contains_any(
        &text,
        &[
            "rate limit",
            "usage limit",
            "quota exceeded",
            "try again later",
            "too many requests",
        ],
    ) {
        Some(QuestionSubtype::RateLimit)
    } else if contains_any(
        &text,
        &[
            "login required",
            "not authenticated",
            "sign in",
            "oauth",
            "api key",
            "authenticate",
        ],
    ) {
        Some(QuestionSubtype::Auth)
    } else if contains_any(&text, &["update available", "new version", "upgrade now"]) {
        Some(QuestionSubtype::Update)
    } else if contains_any(
        &text,
        &[
            "error:",
            "failed",
            "command not found",
            "exit code",
            "traceback",
            "file not found",
        ],
    ) {
        Some(QuestionSubtype::Error)
    } else {
        None
    }
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn ready_prompt(signals: &StreamSignals) -> bool {
    let visible = signals
        .screen
        .lines
        .iter()
        .rev()
        .take(8)
        .map(|line| line.trim())
        .collect::<Vec<_>>();
    let bottom = visible.join("\n").to_lowercase();
    let busy = [
        "esc to interrupt",
        "ctrl+c to stop",
        "running",
        "thinking",
        "composing",
        "editing",
        "reading",
        "globbing",
        "booting",
    ]
    .iter()
    .any(|needle| bottom.contains(needle));
    if busy {
        return false;
    }
    let composer_visible = signals.screen.lines.iter().any(|line| {
        let line = line.trim();
        line.starts_with('❯')
            || line.starts_with('›')
            || line.starts_with("→ add a follow-up")
            || line.starts_with("→ plan, search, build anything")
            || line.contains("ctrl+p commands")
    });
    composer_visible
}

fn collapse_samples(samples: Vec<(u64, Verdict)>, end_ms: u64) -> Vec<ClassifiedSegment> {
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
