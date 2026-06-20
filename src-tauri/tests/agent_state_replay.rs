//! RT2 — agent-state replay + score eval. Parses asciicast v2 `.cast` fixtures,
//! replays them through BOTH the core SignalExtractor (P1a) and the provider
//! `classify_pane_state` path, builds a state timeline, runs golden + negative
//! oracles, and prints a per-state baseline scorecard. One Rust runner exercises
//! both paths (extractor is in-process; classifier loads the bundled JS plugin).
//!
//! Fixtures: tests/fixtures/agent-state/<provider>/<state>/{recording.cast,expected.json}
//! Run: cargo test --test agent_state_replay -- --nocapture  (or evals/agent-state/run.sh)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codeterm_lib::agent::pane_state::{PaneState, PaneStateClassification, PaneStateSnapshot};
use codeterm_lib::agent::provider::AgentProvider;
use codeterm_lib::agent::signal_extractor::{SignalExtractor, StreamSignals, VolatileHint};
use codeterm_lib::plugin::js_runtime::{load_one, JsProvider};
use serde::Deserialize;
use serde_json::Value;

// Reference liveness gate over the SignalExtractor two-clock quiescence. NOT a
// lifecycle state — it stands in for today's coarse "is the turn done / parked"
// signal so the scorecard can count its false positives. P2 replaces it with the
// semantic-veto gate; the eval re-measures and must drive these counts down.
const TICK_MS: u64 = 250; // sample cadence injected into idle gaps to surface stalls
const WAIT_MS: u64 = 800; // byte-idle past this with no settle => "Waiting"
const DONE_MS: u64 = 1500; // semantic-idle past this => the turn looks finished

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Coarse {
    Working,
    Waiting,
    TurnComplete,
}

fn coarse(sig: &StreamSignals) -> Coarse {
    let q = &sig.quiescence;
    if q.byte_idle_ms < WAIT_MS {
        return Coarse::Working; // bytes flowing (incl. spinner) => busy
    }
    if q.semantic_idle_ms >= DONE_MS {
        Coarse::TurnComplete
    } else {
        Coarse::Waiting
    }
}

// --- asciicast v2 ----------------------------------------------------------

enum Event {
    Output { now_ms: u64, bytes: Vec<u8> },
    Resize { now_ms: u64, rows: u16, cols: u16 },
}

impl Event {
    fn now_ms(&self) -> u64 {
        match self {
            Event::Output { now_ms, .. } | Event::Resize { now_ms, .. } => *now_ms,
        }
    }
}

struct Cast {
    width: u16,
    height: u16,
    events: Vec<Event>,
}

fn parse_cast(path: &Path) -> Cast {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());

    let header: Value = serde_json::from_str(lines.next().expect("cast header line"))
        .unwrap_or_else(|e| panic!("cast header in {path:?}: {e}"));
    let width = header["width"].as_u64().unwrap_or(80) as u16;
    let height = header["height"].as_u64().unwrap_or(24) as u16;

    let mut events = Vec::new();
    for line in lines {
        let row: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("cast row {line:?} in {path:?}: {e}"));
        let arr = row.as_array().expect("cast row is an array");
        let now_ms = (arr[0].as_f64().expect("row time") * 1000.0).round() as u64;
        let code = arr[1].as_str().expect("row code");
        let data = arr[2].as_str().expect("row data");
        match code {
            "o" => events.push(Event::Output {
                now_ms,
                bytes: data.as_bytes().to_vec(),
            }),
            "r" => {
                let (cols, rows) = data.split_once('x').expect("resize is COLSxROWS");
                events.push(Event::Resize {
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
    }
}

// --- replay ----------------------------------------------------------------

struct StepResult {
    classify: Option<PaneStateClassification>,
    coarse: Coarse,
    mid_stream: bool, // true while real output still follows this step
}

fn classify_at(provider: &JsProvider, sig: &StreamSignals) -> Option<PaneStateClassification> {
    let snap = PaneStateSnapshot {
        pane_id: "replay".into(),
        agent_type: provider.id().to_string(),
        session_id: None,
        cwd: None,
        screen_text: sig.tail.clone(),
    };
    provider.classify_pane_state(&snap)
}

fn replay(cast: &Cast, provider: &JsProvider) -> Vec<StepResult> {
    let mut ex = SignalExtractor::new(cast.height, cast.width, VolatileHint::default());
    let mut steps = Vec::new();
    let mut last_ms = 0u64;

    let output_after = |i: usize| {
        cast.events[i + 1..]
            .iter()
            .any(|e| matches!(e, Event::Output { .. }))
    };

    for (i, ev) in cast.events.iter().enumerate() {
        // Inject idle ticks across the gap so a mid-stream stall is sampled.
        if !steps.is_empty() {
            let mut t = last_ms + TICK_MS;
            while t < ev.now_ms() {
                let sig = ex.step(b"", t);
                steps.push(StepResult {
                    classify: classify_at(provider, &sig),
                    coarse: coarse(&sig),
                    mid_stream: true, // a later event still follows
                });
                t += TICK_MS;
            }
        }

        match ev {
            Event::Resize { now_ms, rows, cols } => {
                ex.resize(*rows, *cols);
                last_ms = *now_ms;
            }
            Event::Output { now_ms, bytes } => {
                let sig = ex.step(bytes, *now_ms);
                steps.push(StepResult {
                    classify: classify_at(provider, &sig),
                    coarse: coarse(&sig),
                    mid_stream: output_after(i),
                });
                last_ms = *now_ms;
            }
        }
    }

    // Trailing settle ticks: let the clocks reach the parked/quiescent frame.
    let end = last_ms + 2000;
    let mut t = last_ms + TICK_MS;
    while t <= end {
        let sig = ex.step(b"", t);
        steps.push(StepResult {
            classify: classify_at(provider, &sig),
            coarse: coarse(&sig),
            mid_stream: false, // tail: no real output follows
        });
        t += TICK_MS;
    }
    steps
}

// --- oracle ----------------------------------------------------------------

#[derive(Deserialize)]
struct Extracted {
    #[serde(default)]
    keys: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Expected {
    final_state: String,
    #[serde(default)]
    extracted: Option<Extracted>,
    /// States forbidden at ANY mid-stream step. Accepts classify states
    /// (e.g. "rate_limited") AND the coarse axis ("waiting"/"turn_complete").
    #[serde(default)]
    negatives: Vec<String>,
}

fn parse_state(s: &str) -> PaneState {
    serde_json::from_value(Value::String(s.to_string()))
        .unwrap_or_else(|_| panic!("unknown PaneState {s:?}"))
}

fn final_state(steps: &[StepResult]) -> PaneState {
    steps
        .iter()
        .rev()
        .find_map(|s| s.classify.as_ref().map(|c| c.state))
        .unwrap_or(PaneState::Unknown)
}

fn final_keys(steps: &[StepResult]) -> Option<Vec<String>> {
    steps
        .iter()
        .rev()
        .find_map(|s| s.classify.as_ref())
        .and_then(|c| c.action.as_ref())
        .and_then(|a| a.keys.clone())
}

struct FixtureResult {
    name: String,
    truth: PaneState,
    predicted: PaneState,
    false_waiting: usize,
    false_turn_complete: usize,
    failures: Vec<String>,
}

fn run_fixture(name: &str, dir: &Path, provider: &JsProvider) -> FixtureResult {
    let expected: Expected = serde_json::from_str(
        &std::fs::read_to_string(dir.join("expected.json")).expect("read expected.json"),
    )
    .expect("parse expected.json");

    let cast = parse_cast(&dir.join("recording.cast"));
    let steps = replay(&cast, provider);

    let truth = parse_state(&expected.final_state);
    let predicted = final_state(&steps);
    let mut failures = Vec::new();

    // GOLDEN: final detected state == labeled ground truth (+ extracted data).
    if predicted != truth {
        failures.push(format!("final state {predicted:?} != expected {truth:?}"));
    }
    if let Some(ex) = &expected.extracted {
        if let Some(want) = &ex.keys {
            let got = final_keys(&steps);
            if got.as_ref() != Some(want) {
                failures.push(format!("extracted keys {got:?} != expected {want:?}"));
            }
        }
    }

    // NEGATIVE (hard gate): no forbidden state at any mid-stream step.
    let classify_negs: Vec<PaneState> = expected
        .negatives
        .iter()
        .filter(|n| !matches!(n.as_str(), "waiting" | "turn_complete"))
        .map(|n| parse_state(n))
        .collect();
    let forbid_waiting = expected.negatives.iter().any(|n| n == "waiting");
    let forbid_turn_complete = expected.negatives.iter().any(|n| n == "turn_complete");

    let (mut false_waiting, mut false_turn_complete) = (0usize, 0usize);
    for (i, st) in steps.iter().enumerate() {
        if !st.mid_stream {
            continue;
        }
        if let Some(c) = &st.classify {
            if classify_negs.contains(&c.state) {
                failures.push(format!(
                    "mid-stream step {i}: forbidden state {:?}",
                    c.state
                ));
            }
        }
        match st.coarse {
            Coarse::Waiting => {
                false_waiting += 1;
                if forbid_waiting {
                    failures.push(format!("mid-stream step {i}: false Waiting"));
                }
            }
            Coarse::TurnComplete => {
                false_turn_complete += 1;
                if forbid_turn_complete {
                    failures.push(format!("mid-stream step {i}: false turn_complete"));
                }
            }
            Coarse::Working => {}
        }
    }

    FixtureResult {
        name: name.to_string(),
        truth,
        predicted,
        false_waiting,
        false_turn_complete,
        failures,
    }
}

// --- discovery + scorecard -------------------------------------------------

fn fixtures_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("tests/fixtures/agent-state")
}

fn plugin_manifest(provider: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .join(format!("plugins/provider-{provider}/plugin.json"))
}

/// (provider, state-label, fixture-dir) for every dir holding a recording.cast.
fn discover() -> Vec<(String, String, PathBuf)> {
    let mut out = Vec::new();
    let root = fixtures_root();
    let providers =
        std::fs::read_dir(&root).unwrap_or_else(|e| panic!("read fixtures root {root:?}: {e}"));
    for prov in providers.flatten() {
        if !prov.path().is_dir() {
            continue;
        }
        let provider = prov.file_name().to_string_lossy().to_string();
        for state in std::fs::read_dir(prov.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let dir = state.path();
            if dir.join("recording.cast").exists() {
                out.push((
                    provider.clone(),
                    state.file_name().to_string_lossy().to_string(),
                    dir,
                ));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn agent_state_replay_eval() {
    let root = fixtures_root();
    if !root.exists() {
        eprintln!("SKIPPED: no fixtures at {}", root.display());
        return;
    }
    if !plugin_manifest("codex").exists() {
        if std::env::var("CI").is_ok() {
            panic!("agent-state eval requires bundled JS plugins in CI");
        }
        eprintln!("SKIPPED: bundled provider plugins not found (build plugins first)");
        return;
    }

    let mut providers: BTreeMap<String, JsProvider> = BTreeMap::new();
    let mut results = Vec::new();

    for (provider, state, dir) in discover() {
        let p = providers.entry(provider.clone()).or_insert_with(|| {
            load_one(&plugin_manifest(&provider)).expect("load provider plugin")
        });
        results.push(run_fixture(&format!("{provider}/{state}"), &dir, p));
    }

    print_scorecard(&results);

    let failed: Vec<&FixtureResult> = results.iter().filter(|r| !r.failures.is_empty()).collect();
    assert!(
        !results.is_empty(),
        "no agent-state fixtures discovered under {}",
        root.display()
    );
    if !failed.is_empty() {
        for r in &failed {
            eprintln!("FAIL {}", r.name);
            for f in &r.failures {
                eprintln!("    - {f}");
            }
        }
        panic!("{} agent-state fixture(s) failed", failed.len());
    }
}

fn label(s: PaneState) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

fn print_scorecard(results: &[FixtureResult]) {
    // Per-state precision/recall over fixtures (final-state labels).
    let mut tp: BTreeMap<String, usize> = BTreeMap::new();
    let mut fp: BTreeMap<String, usize> = BTreeMap::new();
    let mut f_n: BTreeMap<String, usize> = BTreeMap::new();
    let mut states = std::collections::BTreeSet::new();
    let (mut fw, mut ftc) = (0usize, 0usize);

    for r in results {
        let (truth, predicted) = (label(r.truth), label(r.predicted));
        states.insert(truth.clone());
        states.insert(predicted.clone());
        if r.truth == r.predicted {
            *tp.entry(truth).or_default() += 1;
        } else {
            *fp.entry(predicted).or_default() += 1;
            *f_n.entry(truth).or_default() += 1;
        }
        fw += r.false_waiting;
        ftc += r.false_turn_complete;
    }

    let ratio = |n: usize, d: usize| {
        if d == 0 {
            f64::NAN
        } else {
            n as f64 / d as f64
        }
    };

    println!("\n=== agent-state baseline scorecard ===");
    println!("fixtures: {}", results.len());
    println!("\n{:<22} {:>10} {:>10}", "state", "precision", "recall");
    println!("{}", "-".repeat(44));
    for s in &states {
        let t = *tp.get(s).unwrap_or(&0);
        let precision = ratio(t, t + *fp.get(s).unwrap_or(&0));
        let recall = ratio(t, t + *f_n.get(s).unwrap_or(&0));
        println!("{s:<22} {precision:>10.2} {recall:>10.2}");
    }
    println!("\nfalse Waiting (mid-stream):       {fw}");
    println!("false turn_complete (mid-stream): {ftc}");
    println!("======================================\n");
}
