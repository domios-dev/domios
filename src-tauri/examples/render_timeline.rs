//! L-C timeline renderer (dev tool, anti-overfit centerpiece).
//!
//! Replays a `.cast` (asciicast v2 with our `"i"` input rows) through the SAME
//! production VT engine (`SignalExtractor`), sampling the rendered state at a
//! fixed ~10-15 fps so transient flicker / false cards / status-line churn are
//! visible frame by frame. Emits `timeline.json` consumed by the static HTML
//! replay player (L-D) and by the status-line extractor work (L-F).
//!
//! Usage: cargo run --example render_timeline -- <in.cast> <out.json> \
//!          [--sample-ms N] [--from-ms N|--from-frame N] [--to-ms N|--to-frame N] [--native-picker] [--meta meta.json] [--phases phases.json]
//!
//! Input rows are ANCHORS only — they are NOT fed into the VT screen; they land
//! on a separate `inputs` lane with their cast-clock timestamps.

use std::path::Path;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use codeterm_lib::agent::attributed_screen::detect_picker;
use codeterm_lib::agent::classify_input::{ClassifyInputBuilder, StatusBarRows};
use codeterm_lib::agent::motion_decoder::{MotionDecoder, MotionFrame};
use codeterm_lib::agent::offline_state_classifier::{
    classify_signals, extract_completion_summary, extract_status_line, StatusElapsedTracker,
    TabVerdict, TimelineState,
};
use codeterm_lib::agent::signal_extractor::{SignalExtractor, StreamSignals};
use serde_json::{json, Value};

const DEFAULT_SAMPLE_MS: u64 = 75; // ~13.3 fps

enum Ev {
    Output {
        t_ms: u64,
        bytes: Vec<u8>,
    },
    Resize {
        t_ms: u64,
        rows: u16,
        cols: u16,
    },
    Input {
        t_ms: u64,
        source: String,
        text: Option<String>,
    },
}

fn ev_t(e: &Ev) -> u64 {
    match e {
        Ev::Output { t_ms, .. } | Ev::Resize { t_ms, .. } | Ev::Input { t_ms, .. } => *t_ms,
    }
}

struct Cast {
    width: u16,
    height: u16,
    events: Vec<Ev>,
}

struct DetectorOverlay {
    /// The previous frame's verdict, threaded so the replay runs the SAME stateful
    /// `classify_state` (grace + stuck-working safety net) the live path runs.
    previous: TabVerdict,
    /// The SAME shared builder the live path uses. This renderer has no registry,
    /// so `--status-bar-rows` is required fixture data rather than a default.
    builder: ClassifyInputBuilder,
    /// The SAME elapsed baseline the live path keeps per tab, so replayed and
    /// live observation sequences yield identical progress.
    status_elapsed: StatusElapsedTracker,
}

impl DetectorOverlay {
    fn new(builder: ClassifyInputBuilder) -> Self {
        DetectorOverlay {
            previous: TabVerdict {
                state: TimelineState::Idle,
                subtype: None,
            },
            builder,
            status_elapsed: StatusElapsedTracker::new(),
        }
    }
}

fn parse_cast(path: &Path) -> Cast {
    let text = std::fs::read_to_string(path).expect("read cast");
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Value = serde_json::from_str(lines.next().expect("header")).expect("header json");
    let width = header["width"].as_u64().unwrap_or(80) as u16;
    let height = header["height"].as_u64().unwrap_or(24) as u16;
    // line, and we must not die on it. Skip + count instead of panicking.
    let mut events = Vec::new();
    let mut skipped = 0usize;
    for line in lines {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            skipped += 1;
            continue;
        };
        let Some(arr) = row.as_array() else {
            skipped += 1;
            continue;
        };
        let (Some(t), Some(code)) = (
            arr.first().and_then(|v| v.as_f64()),
            arr.get(1).and_then(|v| v.as_str()),
        ) else {
            skipped += 1;
            continue;
        };
        let t_ms = (t * 1000.0).round() as u64;
        let data = arr.get(2);
        match code {
            "o" => {
                if let Some(s) = data.and_then(|v| v.as_str()) {
                    events.push(Ev::Output {
                        t_ms,
                        bytes: s.as_bytes().to_vec(),
                    });
                }
            }
            "o+b64" => {
                if let Some(b) = data
                    .and_then(|v| v.as_str())
                    .and_then(|s| BASE64.decode(s).ok())
                {
                    events.push(Ev::Output { t_ms, bytes: b });
                }
            }
            "r" => {
                if let Some((c, r)) = data.and_then(|v| v.as_str()).and_then(|s| {
                    let (c, r) = s.split_once('x')?;
                    Some((c.parse::<u16>().ok()?, r.parse::<u16>().ok()?))
                }) {
                    events.push(Ev::Resize {
                        t_ms,
                        cols: c,
                        rows: r,
                    });
                }
            }
            // Input anchor: object payload {source, bytes_b64, text}. Never rendered.
            "i" => events.push(Ev::Input {
                t_ms,
                source: data
                    .and_then(|v| v.get("source"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                text: data
                    .and_then(|v| v.get("text"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            }),
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        eprintln!("note: skipped {skipped} unparseable/partial cast rows (live or truncated cast)");
    }
    Cast {
        width,
        height,
        events,
    }
}

/// Candidate status band = the trailing rows the semantic clock treats as
/// volatile. Surfaced verbatim so the player shows the real structural candidate
/// and L-F can refine the extraction rule against it.
fn volatile_band(sig: &StreamSignals) -> Vec<String> {
    let r = &sig.regions.volatile;
    let last = sig.rows.saturating_sub(1);
    let mut out = Vec::new();
    for row in r.top..=r.bottom.min(last) {
        if let Some(line) = sig.screen.lines.get(row as usize) {
            let t = line.trim_end();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }
    out
}

fn max_phase_to_ms(phases: Option<&Value>) -> Option<u64> {
    let phases = phases?;
    let phase_rows = phases
        .get("phases")
        .and_then(|v| v.as_array())
        .or_else(|| phases.as_array())?;
    phase_rows
        .iter()
        .filter_map(|phase| phase.get("to_ms").and_then(|v| v.as_u64()))
        .max()
}

/// Per-frame detector overlay. The STATE is the production verdict: this calls
/// the exact `classify_signals`/`classify_state` the live path runs, threading
/// `overlay.previous` so the stateful grace + stuck-working logic replays
/// faithfully (NOT a parallel status-line/hold-decay approximation). The
/// status_line / summary fields are display-only, surfaced per verdict state to
/// mirror `service::output::recompute_agent_state`.
fn detector_json(
    extractor: &SignalExtractor,
    sig: &StreamSignals,
    card: Value,
    overlay: &mut DetectorOverlay,
) -> Value {
    let verdict = classify_signals(
        extractor,
        sig,
        overlay.previous,
        &overlay.builder,
        &mut overlay.status_elapsed,
    );
    overlay.previous = verdict;

    let status = extract_status_line(&sig.screen.lines);
    let working = matches!(verdict.state, TimelineState::Working);
    let idle = matches!(verdict.state, TimelineState::Idle);
    let status_line = working
        .then(|| status.as_ref().map(|s| s.line.clone()))
        .flatten();
    let phase = working
        .then(|| status.as_ref().and_then(|s| s.phase.clone()))
        .flatten();
    let elapsed_ms = working
        .then(|| status.as_ref().and_then(|s| s.elapsed_ms))
        .flatten();
    // separate muted line — NOT the working line.
    let last_turn_summary = idle
        .then(|| extract_completion_summary(&sig.screen.lines).map(|s| s.line))
        .flatten();
    let state = match verdict.state {
        TimelineState::Working => "working",
        TimelineState::Idle => "idle",
        TimelineState::Question => "question",
    };
    json!({
        "state": state,
        "working": working,
        "subtype": verdict.subtype,
        "status_line": status_line,
        "phase": phase,
        "elapsed_ms": elapsed_ms,
        "last_turn_summary": last_turn_summary,
        "card": card,
    })
}

fn motion_json(motion: &MotionFrame) -> Value {
    json!({
        "kind": motion.kind,
        "shift_up": motion.shift_up,
        "promoted": motion.promoted,
        "classes": {
            "shifted": motion.classes.shifted,
            "appended": motion.classes.appended,
            "mutated": motion.classes.mutated,
            "volatile": motion.classes.volatile,
        },
        "volatile_region": motion.volatile_region,
        "turn_seq": motion.turn_seq,
        "turn_frozen": motion.turn_frozen,
        "turn_live": motion.turn_live,
    })
}

fn frame_json(
    t_ms: u64,
    frame_index: u64,
    extractor: &SignalExtractor,
    sig: &StreamSignals,
    motion: &MotionFrame,
    include_native_picker: bool,
    overlay: &mut DetectorOverlay,
) -> Value {
    let screen: Vec<String> = sig
        .screen
        .lines
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect();
    let mut detector = detector_json(extractor, sig, Value::Null, overlay);
    if include_native_picker {
        let native_picker = detect_picker(&extractor.attributed_screen())
            .map(|picker| {
                json!({
                    "question": picker.question,
                    "options": picker.options,
                })
            })
            .unwrap_or(Value::Null);
        if let Some(obj) = detector.as_object_mut() {
            obj.insert("native_picker".to_string(), native_picker);
        }
    }
    json!({
        "t_ms": t_ms,
        "frame_index": frame_index,
        "screen": screen,
        "cursor": { "row": sig.cursor.row, "col": sig.cursor.col, "visible": sig.cursor.visible },
        "decset": {
            "alt_screen": sig.input_mode.alt_screen,
            "cursor_hidden": sig.input_mode.cursor_hidden,
            "bracketed_paste": sig.input_mode.bracketed_paste,
        },
        "quiescence": {
            "byte_idle_ms": sig.quiescence.byte_idle_ms,
            "semantic_idle_ms": sig.quiescence.semantic_idle_ms,
            "volatile_only": sig.quiescence.volatile_only,
        },
        "changed_rows": sig.delta.changed_rows,
        "picker_active": sig.picker_active,
        "volatile_rows": volatile_band(sig),
        "motion": motion_json(motion),
        "detector": detector,
    })
}

/// Advance the extractor with empty steps at each sample tick `<= t`, emitting a
/// frame of the time-advanced state. Empty steps grow the idle clocks exactly as
/// production does between PTY chunks — without them the state never settles.
fn sample_up_to(
    t: u64,
    ex: &mut SignalExtractor,
    motion: &mut MotionDecoder,
    frames: &mut Vec<Value>,
    next: &mut u64,
    step: u64,
    window_from_ms: u64,
    window_to_ms: u64,
    frame_index: &mut u64,
    include_native_picker: bool,
    overlay: &mut DetectorOverlay,
) {
    while *next <= t {
        let sig = ex.step(b"", *next);
        // Motion state must advance on every sample tick, emitted or not.
        let motion_frame =
            motion.step_with_cursor(&sig.screen.lines, Some(sig.cursor.row as usize));
        *frame_index += 1;
        if *next >= window_from_ms && *next <= window_to_ms {
            frames.push(frame_json(
                *next,
                *frame_index,
                ex,
                &sig,
                &motion_frame,
                include_native_picker,
                overlay,
            ));
        }
        *next += step;
    }
}

fn read_json(path: &str) -> Option<Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let pos: Vec<&String> = argv[1..].iter().filter(|a| !a.starts_with("--")).collect();
    if pos.len() < 2 {
        eprintln!("usage: render_timeline <in.cast> <out.json> --status-bar-rows N [--sample-ms N] [--from-ms N|--from-frame N] [--to-ms N|--to-frame N] [--native-picker] [--meta m.json] [--phases p.json]");
        std::process::exit(2);
    }
    let in_path = Path::new(pos[0]);
    let out_path = Path::new(pos[1]);
    let flag = |name: &str| {
        argv.iter()
            .position(|a| a == name)
            .and_then(|i| argv.get(i + 1))
            .cloned()
    };
    let sample_ms = flag("--sample-ms")
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_SAMPLE_MS);
    let meta = flag("--meta").and_then(|p| read_json(&p));
    let phases = flag("--phases").and_then(|p| read_json(&p));
    let include_native_picker = argv.iter().any(|a| a == "--native-picker");
    // Required, no default: replaying with a fictional band would render a
    // timeline for a program nobody runs.
    let Some(status_bar_rows) = flag("--status-bar-rows").and_then(|s| s.parse::<u16>().ok())
    else {
        eprintln!("--status-bar-rows N is required: the replayed provider's chrome band");
        std::process::exit(2);
    };
    let builder = ClassifyInputBuilder::for_rows(StatusBarRows::fixture(status_bar_rows));

    let cast = parse_cast(in_path);
    let cast_last_t = cast.events.iter().map(ev_t).max().unwrap_or(0);
    let full_end = (cast_last_t + 2000).max(max_phase_to_ms(phases.as_ref()).unwrap_or(0));
    let frame_to_ms = |frame: u64| frame.saturating_sub(1) * sample_ms;
    let window_from_ms = flag("--from-ms")
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            flag("--from-frame")
                .and_then(|s| s.parse().ok())
                .map(frame_to_ms)
        })
        .unwrap_or(0);
    let window_to_ms = flag("--to-ms")
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            flag("--to-frame")
                .and_then(|s| s.parse().ok())
                .map(frame_to_ms)
        })
        .unwrap_or(full_end);
    if window_from_ms > window_to_ms {
        eprintln!("--from-ms must be <= --to-ms");
        std::process::exit(2);
    }
    // produced a screen.txt oracle). Defaults to the cast header geometry.
    let rows = flag("--rows")
        .and_then(|s| s.parse().ok())
        .unwrap_or(cast.height);
    let cols = flag("--cols")
        .and_then(|s| s.parse().ok())
        .unwrap_or(cast.width);
    let mut ex = SignalExtractor::new(rows, cols, builder.volatile_hint());
    let mut motion = MotionDecoder::new();

    let mut frames: Vec<Value> = Vec::new();
    let mut inputs: Vec<Value> = Vec::new();
    let mut next_sample = 0u64;
    let mut frame_index = 0u64;
    let mut overlay = DetectorOverlay::new(builder);

    for ev in &cast.events {
        let t = ev_t(ev);
        if t > window_to_ms && next_sample > window_to_ms {
            break;
        }
        // Advance the clock + sample BEFORE applying this event.
        sample_up_to(
            t.saturating_sub(1),
            &mut ex,
            &mut motion,
            &mut frames,
            &mut next_sample,
            sample_ms,
            window_from_ms,
            window_to_ms,
            &mut frame_index,
            include_native_picker,
            &mut overlay,
        );
        match ev {
            Ev::Output { bytes, t_ms } => {
                ex.step(bytes, *t_ms);
            }
            Ev::Resize { rows, cols, .. } => {
                ex.resize(*rows, *cols);
            }
            Ev::Input { t_ms, source, text } => {
                // A submit key starts a new turn window for the stream extractor.
                if text.as_deref().is_some_and(|t| t.contains('\r')) {
                    motion.begin_turn();
                }
                if *t_ms >= window_from_ms && *t_ms <= window_to_ms {
                    inputs.push(json!({ "t_ms": t_ms, "source": source, "text": text }));
                }
            }
        }
    }
    // silent idle hold, so there may be no cast event inside it.
    let end = window_to_ms;
    sample_up_to(
        end.min(window_to_ms),
        &mut ex,
        &mut motion,
        &mut frames,
        &mut next_sample,
        sample_ms,
        window_from_ms,
        window_to_ms,
        &mut frame_index,
        include_native_picker,
        &mut overlay,
    );

    let m = meta.unwrap_or(Value::Null);
    let doc = json!({
        "provider": m.get("provider").cloned().unwrap_or(Value::Null),
        "scenario": m.get("scenario").or_else(|| m.get("id")).cloned().unwrap_or(Value::Null),
        "expected_phases": phases.unwrap_or_else(|| json!([])),
        "meta": {
            "cast": in_path.display().to_string(),
            "width": cols,
            "height": rows,
            "cast_width": cast.width,
            "cast_height": cast.height,
            "sample_ms": sample_ms,
            "fps": (1000.0 / sample_ms as f64 * 10.0).round() / 10.0,
            "frames": frames.len(),
            "duration_ms": full_end,
            "window_from_ms": window_from_ms,
            "window_to_ms": window_to_ms,
            "global_frame_count": frame_index,
        },
        "inputs": inputs,
        "frames": frames,
    });
    std::fs::write(out_path, serde_json::to_string(&doc).expect("serialize")).expect("write out");
    eprintln!(
        "wrote {} ({} frames @ {}ms, {} input anchors, window {}..{}ms)",
        out_path.display(),
        frames.len(),
        sample_ms,
        inputs.len(),
        window_from_ms,
        window_to_ms
    );
}
