use std::path::{Path, PathBuf};

use codeterm_lib::agent::offline_state_classifier::{
    classify_timeline, render_signals_at, CastEvent, CastTimelineInput, ClassifiedSegment,
    QuestionSubtype, TimelineState, DONE_MS,
};
use serde::Deserialize;
use serde_json::Value;

const MIN_CAST_ROWS: usize = 50;
const MIN_CAST_BYTES: u64 = 40_000;

#[derive(Clone, Debug)]
struct CastStats {
    rows: usize,
    bytes: u64,
}

#[derive(Clone, Debug)]
struct Cast {
    width: u16,
    height: u16,
    events: Vec<CastEvent>,
    stats: CastStats,
}

#[derive(Clone, Debug, Deserialize)]
struct StateTimeline {
    segments: Vec<LabeledSegment>,
}

#[derive(Clone, Debug, Deserialize)]
struct LabeledSegment {
    from_ms: u64,
    to_ms: u64,
    state: TimelineState,
    #[serde(default)]
    subtype: Option<QuestionSubtype>,
}

#[derive(Clone, Debug)]
struct DumpVerdict {
    case: String,
    ok: bool,
    details: Vec<String>,
}

pub fn state_dumps_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testbench/state-dumps"
    ))
}

pub fn run_state_doctor(root: &Path) -> bool {
    let dumps = discover_state_dumps(root);
    println!("\n=== state dump doctor ===");
    print_rule_set();
    println!("\n{:<42} {:<4} details", "dump", "rule");
    println!("{}", "-".repeat(118));

    let mut all_ok = true;
    for dir in dumps {
        let verdict = inspect_dump(root, &dir);
        all_ok &= verdict.ok;
        let status = if verdict.ok { "OK" } else { "BAD" };
        println!(
            "{:<42} {:<4} {}",
            verdict.case,
            status,
            verdict.details.join("; ")
        );
    }
    all_ok
}

fn print_rule_set() {
    println!("rules:");
    println!(
        "  size floor: session.cast has >= {MIN_CAST_ROWS} rows and >= {MIN_CAST_BYTES} bytes"
    );
    println!(
        "  thin reuse: BAD when below both floors and no classifier or labeled transition proves span"
    );
    println!("  shape: code-then-idle=working then idle; multi-turn=>=4 alternating working/idle");
    println!(
        "  shape: model_choice=question:model_choice; model-picker=real transition into question"
    );
    println!("  shape: error=question:error; approval=question:permission; update-prompt=question:update");
    println!("  boundary: working->idle must re-render with settled quiescence; ->question must show picker signal");
}

fn inspect_dump(root: &Path, dir: &Path) -> DumpVerdict {
    let case = dir
        .strip_prefix(root)
        .unwrap_or(dir)
        .to_string_lossy()
        .replace('\\', "/");
    let scenario = dir.file_name().unwrap_or_default().to_string_lossy();
    let timeline = parse_timeline(&dir.join("timeline.json"));
    let cast = parse_cast(&dir.join("session.cast"));
    let input = CastTimelineInput {
        width: cast.width,
        height: cast.height,
        events: cast.events.clone(),
    };
    let predicted = classify_timeline(&input);

    let mut problems = Vec::new();
    let transitions = transition_count(&predicted);
    let labeled_transitions = labeled_transition_count(&timeline.segments);
    if cast.stats.rows < MIN_CAST_ROWS
        && cast.stats.bytes < MIN_CAST_BYTES
        && (transitions == 0 || labeled_transitions == 0)
    {
        problems.push(format!(
            "thin reused cast: {} rows, {} bytes, {} classifier transition(s), {} labeled transition(s)",
            cast.stats.rows, cast.stats.bytes, transitions, labeled_transitions
        ));
    }
    problems.extend(validate_shape(&scenario, &timeline.segments));
    problems.extend(validate_boundaries(&input, &timeline.segments));

    let details = if problems.is_empty() {
        vec![format!(
            "{} rows, {} bytes, {} classifier transition(s)",
            cast.stats.rows, cast.stats.bytes, transitions
        )]
    } else {
        problems
    };

    DumpVerdict {
        case,
        ok: details.len() == 1 && !details[0].contains("BAD_SENTINEL"),
        details,
    }
    .normalized()
}

impl DumpVerdict {
    fn normalized(mut self) -> Self {
        self.ok = !self.details.iter().any(|detail| {
            detail.starts_with("thin ")
                || detail.starts_with("shape ")
                || detail.starts_with("boundary ")
        });
        self
    }
}

fn validate_shape(scenario: &str, segments: &[LabeledSegment]) -> Vec<String> {
    let mut problems = Vec::new();
    if segments.is_empty() {
        return vec!["shape missing timeline segments".into()];
    }
    for segment in segments {
        if segment.to_ms <= segment.from_ms {
            problems.push(format!(
                "shape non-positive segment {}..{}",
                segment.from_ms, segment.to_ms
            ));
        }
    }

    match scenario {
        "code-then-idle" => {
            if !has_ordered_states(segments, &[TimelineState::Working, TimelineState::Idle]) {
                problems.push("shape code-then-idle lacks working->idle".into());
            }
        }
        "multi-turn" => {
            let states = segments.iter().map(|s| s.state).collect::<Vec<_>>();
            let alternates = states.windows(2).all(|pair| pair[0] != pair[1])
                && states
                    .iter()
                    .all(|state| matches!(state, TimelineState::Working | TimelineState::Idle));
            if states.len() < 4 || !alternates {
                problems.push(format!(
                    "shape multi-turn expected >=4 alternating working/idle, got {} segment(s)",
                    states.len()
                ));
            }
        }
        "model_choice" | "model-choice" => {
            if !has_question_subtype(segments, QuestionSubtype::ModelChoice) {
                problems.push("shape model_choice lacks question:model_choice".into());
            }
        }
        "model-picker" => {
            if !contains_state(segments, TimelineState::Question) {
                problems.push("shape model-picker lacks question segment".into());
            }
            if segments.len() < 2 || !has_boundary_into(segments, TimelineState::Question) {
                problems.push("shape model-picker lacks real transition into question".into());
            }
        }
        "error" => {
            if !has_question_subtype(segments, QuestionSubtype::Error) {
                problems.push("shape error lacks question:error segment".into());
            }
        }
        "approval" => {
            if !has_question_subtype(segments, QuestionSubtype::Permission) {
                problems.push("shape approval lacks question:permission segment".into());
            }
        }
        "update-prompt" => {
            if !has_question_subtype(segments, QuestionSubtype::Update) {
                problems.push("shape update-prompt lacks question:update segment".into());
            }
        }
        "connect-picker" => {
            if !has_question_subtype(segments, QuestionSubtype::Auth) {
                problems.push("shape connect-picker lacks question:auth segment".into());
            }
        }
        _ => {}
    }
    problems
}

fn validate_boundaries(input: &CastTimelineInput, segments: &[LabeledSegment]) -> Vec<String> {
    let mut problems = Vec::new();
    for pair in segments.windows(2) {
        let before = &pair[0];
        let after = &pair[1];
        let boundary = after.from_ms;
        if after.state == TimelineState::Idle && before.state == TimelineState::Working {
            let signals = render_signals_at(input, boundary);
            if signals.semantic_idle_ms < DONE_MS || !signals.ready_prompt {
                problems.push(format!(
                    "boundary {}ms working->idle lacks settled quiescence (semantic_idle={} ready={})",
                    boundary, signals.semantic_idle_ms, signals.ready_prompt
                ));
            }
        }
        if after.state == TimelineState::Question {
            let signals = render_signals_at(input, boundary);
            if !signals.picker_active && !signals.picker_detected {
                problems.push(format!(
                    "boundary {}ms ->question lacks picker signal (active={} detected={})",
                    boundary, signals.picker_active, signals.picker_detected
                ));
            }
        }
    }
    problems
}

fn has_ordered_states(segments: &[LabeledSegment], want: &[TimelineState]) -> bool {
    let mut pos = 0;
    for segment in segments {
        if pos < want.len() && segment.state == want[pos] {
            pos += 1;
        }
    }
    pos == want.len()
}

fn has_boundary_into(segments: &[LabeledSegment], state: TimelineState) -> bool {
    segments
        .windows(2)
        .any(|pair| pair[0].state != state && pair[1].state == state)
}

fn contains_state(segments: &[LabeledSegment], state: TimelineState) -> bool {
    segments.iter().any(|segment| segment.state == state)
}

fn has_question_subtype(segments: &[LabeledSegment], subtype: QuestionSubtype) -> bool {
    segments
        .iter()
        .any(|segment| segment.state == TimelineState::Question && segment.subtype == Some(subtype))
}

fn transition_count(segments: &[ClassifiedSegment]) -> usize {
    segments
        .windows(2)
        .filter(|pair| pair[0].state != pair[1].state || pair[0].subtype != pair[1].subtype)
        .count()
}

fn labeled_transition_count(segments: &[LabeledSegment]) -> usize {
    segments
        .windows(2)
        .filter(|pair| pair[0].state != pair[1].state || pair[0].subtype != pair[1].subtype)
        .count()
}

fn discover_state_dumps(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    visit_state_dumps(root, &mut out);
    out.sort();
    out
}

fn visit_state_dumps(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.join("session.cast").exists() && path.join("timeline.json").exists() {
                out.push(path);
            } else {
                visit_state_dumps(&path, out);
            }
        }
    }
}

fn parse_timeline(path: &Path) -> StateTimeline {
    serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
    )
    .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn parse_cast(path: &Path) -> Cast {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next().expect("cast header line"))
        .unwrap_or_else(|e| panic!("cast header in {path:?}: {e}"));
    let width = header["width"].as_u64().unwrap_or(80) as u16;
    let height = header["height"].as_u64().unwrap_or(24) as u16;
    let mut events = Vec::new();
    let mut rows = 0;
    for line in lines {
        rows += 1;
        let row: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("cast row {line:?} in {path:?}: {e}"));
        let arr = row.as_array().expect("cast row is an array");
        let now_ms = (arr[0].as_f64().expect("row time") * 1000.0).round() as u64;
        let code = arr[1].as_str().expect("row code");
        let data = arr[2].as_str().expect("row data");
        match code {
            "o" => events.push(CastEvent::Output {
                now_ms,
                bytes: data.as_bytes().to_vec(),
            }),
            "r" => {
                let (cols, rows) = data.split_once('x').expect("resize is COLSxROWS");
                events.push(CastEvent::Resize {
                    now_ms,
                    cols: cols.parse().expect("resize cols"),
                    rows: rows.parse().expect("resize rows"),
                });
            }
            other => panic!("unknown cast event code {other:?} in {path:?}"),
        }
    }
    Cast {
        width,
        height,
        events,
        stats: CastStats {
            rows,
            bytes: text.len() as u64,
        },
    }
}
