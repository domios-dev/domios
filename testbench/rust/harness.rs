use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use codeterm_lib::agent::command_detection::CommandDetection;
use codeterm_lib::agent::detector;
use codeterm_lib::agent::pane_state::{PaneState, PaneStateClassification, PaneStateSnapshot};
use codeterm_lib::agent::provider::AgentProvider;
use codeterm_lib::agent::semantic_marker::{SemanticEvent, SemanticMarkerParser};
use codeterm_lib::agent::signal_extractor::{SignalExtractor, StreamSignals, VolatileHint};
use codeterm_lib::plugin::js_runtime::{load_one, JsProvider};
use serde::Deserialize;
use serde_json::{json, Value};

const TICK_MS: u64 = 250;
const WAIT_MS: u64 = 800;
const DONE_MS: u64 = 1500;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Coarse {
    Working,
    Waiting,
    TurnComplete,
}

#[derive(Clone, Debug)]
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

#[derive(Clone, Debug)]
struct Cast {
    width: u16,
    height: u16,
    events: Vec<Event>,
}

#[derive(Clone, Debug)]
struct StepResult {
    classify: Option<PaneStateClassification>,
    coarse: Coarse,
    mid_stream: bool,
    signals: StreamSignals,
}

#[derive(Debug, Deserialize)]
struct Extracted {
    #[serde(default)]
    keys: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct LegacyAgentStateExpected {
    final_state: String,
    #[serde(default)]
    extracted: Option<Extracted>,
    #[serde(default)]
    negatives: Vec<String>,
    #[serde(default = "default_true")]
    gating: bool,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Unit {
    AgentStateReplay,
    ProviderScreen,
}

#[derive(Debug, Deserialize)]
struct SnapshotExpected {
    provider: String,
    #[serde(default)]
    unit: Option<Unit>,
    #[serde(default)]
    expected: Option<Value>,
    #[serde(default)]
    final_state: Option<String>,
    #[serde(default)]
    extracted: Option<Extracted>,
    #[serde(default)]
    negatives: Vec<String>,
    #[serde(default = "default_true")]
    gating: bool,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug)]
pub struct CaseResult {
    pub name: String,
    pub unit: String,
    pub gating: bool,
    pub failures: Vec<String>,
    pub details: Value,
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

pub fn snapshots_root() -> PathBuf {
    workspace_root().join("testbench/snapshots")
}

pub fn plugin_manifest(provider: &str) -> PathBuf {
    workspace_root().join(format!("plugins/provider-{provider}/plugin.json"))
}

pub fn load_provider(provider: &str) -> JsProvider {
    load_one(&plugin_manifest(provider)).unwrap_or_else(|e| panic!("load {provider} plugin: {e}"))
}

pub fn discover_cases(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    visit(root, &mut out);
    out.sort();
    out
}

fn visit(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.join("expected.json").exists() {
                out.push(path);
            } else {
                visit(&path, out);
            }
        }
    }
}

pub fn run_case(
    root: &Path,
    dir: &Path,
    providers: &mut BTreeMap<String, JsProvider>,
) -> CaseResult {
    let expected_path = dir.join("expected.json");
    let raw: Value = serde_json::from_str(
        &std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display())),
    )
    .unwrap_or_else(|e| panic!("parse {}: {e}", expected_path.display()));
    let expected: SnapshotExpected = serde_json::from_value(raw.clone())
        .unwrap_or_else(|e| panic!("decode {}: {e}", expected_path.display()));
    let name = dir
        .strip_prefix(root)
        .unwrap_or(dir)
        .to_string_lossy()
        .replace('\\', "/");

    match expected.unit.unwrap_or_else(|| infer_unit(dir, &raw)) {
        Unit::AgentStateReplay => {
            let legacy = LegacyAgentStateExpected {
                final_state: expected
                    .final_state
                    .unwrap_or_else(|| raw["final_state"].as_str().unwrap_or("unknown").into()),
                extracted: expected.extracted,
                negatives: expected.negatives,
                gating: expected.gating,
                source: expected.source,
            };
            let provider = providers
                .entry(expected.provider.clone())
                .or_insert_with(|| load_provider(&expected.provider));
            run_agent_state_case(&name, dir, provider, legacy)
        }
        Unit::ProviderScreen => {
            let provider = providers
                .entry(expected.provider.clone())
                .or_insert_with(|| load_provider(&expected.provider));
            run_provider_screen_case(&name, dir, provider, expected)
        }
    }
}

fn infer_unit(dir: &Path, raw: &Value) -> Unit {
    if raw.get("unit").is_none()
        && (dir.join("raw.cast").exists() || dir.join("recording.cast").exists())
    {
        Unit::AgentStateReplay
    } else {
        Unit::ProviderScreen
    }
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

fn coarse(sig: &StreamSignals) -> Coarse {
    let q = &sig.quiescence;
    if q.byte_idle_ms < WAIT_MS {
        return Coarse::Working;
    }
    if q.semantic_idle_ms >= DONE_MS {
        Coarse::TurnComplete
    } else {
        Coarse::Waiting
    }
}

fn classify_at(provider: &JsProvider, sig: &StreamSignals) -> Option<PaneStateClassification> {
    provider.classify_pane_state(&PaneStateSnapshot {
        pane_id: "testbench".into(),
        agent_type: provider.id().to_string(),
        session_id: None,
        cwd: None,
        screen_text: sig.tail.clone(),
    })
}

fn replay(cast: &Cast, provider: &JsProvider) -> Vec<StepResult> {
    let mut extractor = SignalExtractor::new(cast.height, cast.width, VolatileHint::default());
    let mut steps = Vec::new();
    let mut last_ms = 0u64;

    let output_after = |i: usize| {
        cast.events[i + 1..]
            .iter()
            .any(|e| matches!(e, Event::Output { .. }))
    };

    for (i, ev) in cast.events.iter().enumerate() {
        if !steps.is_empty() {
            let mut t = last_ms + TICK_MS;
            while t < ev.now_ms() {
                let sig = extractor.step(b"", t);
                steps.push(StepResult {
                    classify: classify_at(provider, &sig),
                    coarse: coarse(&sig),
                    mid_stream: true,
                    signals: sig,
                });
                t += TICK_MS;
            }
        }

        match ev {
            Event::Resize { now_ms, rows, cols } => {
                extractor.resize(*rows, *cols);
                last_ms = *now_ms;
            }
            Event::Output { now_ms, bytes } => {
                let sig = extractor.step(bytes, *now_ms);
                steps.push(StepResult {
                    classify: classify_at(provider, &sig),
                    coarse: coarse(&sig),
                    mid_stream: output_after(i),
                    signals: sig,
                });
                last_ms = *now_ms;
            }
        }
    }

    let end = last_ms + 2000;
    let mut t = last_ms + TICK_MS;
    while t <= end {
        let sig = extractor.step(b"", t);
        steps.push(StepResult {
            classify: classify_at(provider, &sig),
            coarse: coarse(&sig),
            mid_stream: false,
            signals: sig,
        });
        t += TICK_MS;
    }
    steps
}

fn run_agent_state_case(
    name: &str,
    dir: &Path,
    provider: &JsProvider,
    expected: LegacyAgentStateExpected,
) -> CaseResult {
    let cast_path = if dir.join("raw.cast").exists() {
        dir.join("raw.cast")
    } else {
        dir.join("recording.cast")
    };
    let cast = parse_cast(&cast_path);
    let marker_summary = analyze_markers(&cast);
    let steps = replay(&cast, provider);
    let truth = parse_state(&expected.final_state);
    let predicted = final_state(&steps);
    let mut failures = Vec::new();

    if predicted != truth {
        failures.push(format!(
            "final state {} != expected {}",
            label(predicted),
            label(truth)
        ));
    }
    if let Some(ex) = &expected.extracted {
        if let Some(want) = &ex.keys {
            let got = final_keys(&steps);
            if got.as_ref() != Some(want) {
                failures.push(format!("extracted keys {got:?} != expected {want:?}"));
            }
        }
    }

    let classify_negs: Vec<PaneState> = expected
        .negatives
        .iter()
        .filter(|n| !matches!(n.as_str(), "waiting" | "turn_complete"))
        .map(|n| parse_state(n))
        .collect();
    let forbid_waiting = expected.negatives.iter().any(|n| n == "waiting");
    let forbid_turn_complete = expected.negatives.iter().any(|n| n == "turn_complete");
    let (mut false_waiting, mut false_turn_complete) = (0usize, 0usize);

    for (i, step) in steps.iter().enumerate() {
        if !step.mid_stream {
            continue;
        }
        if let Some(c) = &step.classify {
            if classify_negs.contains(&c.state) {
                failures.push(format!(
                    "mid-stream step {i}: forbidden state {}",
                    label(c.state)
                ));
            }
        }
        match step.coarse {
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

    let final_tail = steps
        .last()
        .map(|s| s.signals.tail.clone())
        .unwrap_or_default();
    CaseResult {
        name: name.into(),
        unit: "agent_state_replay".into(),
        gating: expected.gating,
        failures,
        details: json!({
            "truth": label(truth),
            "predicted": label(predicted),
            "falseWaiting": false_waiting,
            "falseTurnComplete": false_turn_complete,
            "source": expected.source,
            "semanticMarkers": marker_summary,
            "finalTailChars": final_tail.chars().count(),
        }),
    }
}

fn run_provider_screen_case(
    name: &str,
    dir: &Path,
    provider: &JsProvider,
    case: SnapshotExpected,
) -> CaseResult {
    let text = std::fs::read_to_string(dir.join("screen.txt"))
        .unwrap_or_else(|e| panic!("read {}/screen.txt: {e}", dir.display()));
    let actual = provider_screen_actual(provider, &text);
    let expected = case
        .expected
        .unwrap_or_else(|| panic!("{name}: provider_screen expected object missing"));
    let mut failures = Vec::new();
    compare_value("", &expected, &actual, &mut failures);
    CaseResult {
        name: name.into(),
        unit: "provider_screen".into(),
        gating: case.gating,
        failures,
        details: json!({
            "expected": expected,
            "actual": actual,
        }),
    }
}

pub fn provider_screen_actual(provider: &JsProvider, text: &str) -> Value {
    let prompt = provider.parse_prompt(text);
    let classify = provider.classify_pane_state(&PaneStateSnapshot {
        pane_id: "testbench".into(),
        agent_type: provider.id().to_string(),
        session_id: None,
        cwd: None,
        screen_text: text.to_string(),
    });
    json!({
        "hasEventMarkers": provider.has_event_markers(text),
        "detectEvents": provider.detect_events(text).iter().map(|e| format!("{e:?}")).collect::<Vec<_>>(),
        "genericPromptDetected": detector::detect_interactive_prompt(text),
        "parsePrompt": prompt,
        "classifyPaneState": classify,
    })
}

fn analyze_markers(cast: &Cast) -> Value {
    let mut parser = SemanticMarkerParser::new();
    let mut commands = CommandDetection::new();
    let mut semantic_events = Vec::new();
    let mut lifecycle = Vec::new();

    for event in &cast.events {
        if let Event::Output { bytes, .. } = event {
            for semantic in parser.feed(bytes) {
                semantic_events.push(format_semantic_event(&semantic));
                if let Some(change) = commands.apply(semantic) {
                    lifecycle.push(json!(change));
                }
            }
        }
    }

    json!({
        "events": semantic_events,
        "commands": commands.commands(),
        "cwd": commands.cwd(),
        "lifecycle": lifecycle,
    })
}

fn format_semantic_event(event: &SemanticEvent) -> String {
    match event {
        SemanticEvent::PromptStart => "prompt_start".into(),
        SemanticEvent::PromptEnd => "prompt_end".into(),
        SemanticEvent::CommandStart => "command_start".into(),
        SemanticEvent::CommandEnd { exit_code } => format!("command_end:{exit_code:?}"),
        SemanticEvent::CommandLine { text, nonce } => format!("command_line:{text}:{nonce:?}"),
        SemanticEvent::Property { key, value } => format!("property:{key}={value}"),
        SemanticEvent::Cwd { path } => format!("cwd:{path}"),
    }
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

pub fn print_results(results: &[CaseResult]) {
    println!("\n=== snapshot testbench ===");
    println!("{:<56} {:<20} {:<8} {}", "case", "unit", "bucket", "result");
    println!("{}", "-".repeat(100));
    for result in results {
        let bucket = if result.gating { "gating" } else { "baseline" };
        let status = if result.failures.is_empty() {
            "ok"
        } else {
            "FAIL"
        };
        println!(
            "{:<56} {:<20} {:<8} {}",
            result.name, result.unit, bucket, status
        );
        for failure in &result.failures {
            println!("  - {failure}");
        }
    }

    let mut by_unit = BTreeMap::<&str, usize>::new();
    for result in results {
        *by_unit.entry(&result.unit).or_default() += 1;
    }
    println!("\nunits:");
    for (unit, count) in by_unit {
        println!("  {unit}: {count}");
    }
}

pub fn assert_gating_green(results: &[CaseResult]) {
    let failures: Vec<&CaseResult> = results
        .iter()
        .filter(|r| r.gating && !r.failures.is_empty())
        .collect();
    if failures.is_empty() {
        return;
    }
    for result in &failures {
        eprintln!("\nFAIL {}", result.name);
        for failure in &result.failures {
            eprintln!("  - {failure}");
        }
        eprintln!("{}", serde_json::to_string_pretty(&result.details).unwrap());
    }
    panic!("{} gating snapshot case(s) failed", failures.len());
}

pub fn print_scorecard(results: &[CaseResult]) {
    let mut states = BTreeSet::new();
    let mut counts = BTreeMap::<String, usize>::new();
    for result in results {
        if result.unit != "agent_state_replay" {
            continue;
        }
        if let Some(predicted) = result.details["predicted"].as_str() {
            states.insert(predicted.to_string());
            *counts.entry(predicted.to_string()).or_default() += 1;
        }
    }
    if states.is_empty() {
        return;
    }
    println!("\nagent-state predicted states:");
    for state in states {
        println!("  {state}: {}", counts.get(&state).copied().unwrap_or(0));
    }
}

fn compare_value(path: &str, expected: &Value, actual: &Value, failures: &mut Vec<String>) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (key, ev) in e {
                let child = if path.is_empty() {
                    key.to_string()
                } else {
                    format!("{path}.{key}")
                };
                match a.get(key) {
                    Some(av) => compare_value(&child, ev, av, failures),
                    None => failures.push(format!("{child}: missing actual key")),
                }
            }
        }
        _ if expected != actual => failures.push(format!(
            "{path}: expected {}, got {}",
            compact(expected),
            compact(actual)
        )),
        _ => {}
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<json>".into())
}

fn parse_state(s: &str) -> PaneState {
    serde_json::from_value(Value::String(s.to_string()))
        .unwrap_or_else(|_| panic!("unknown PaneState {s:?}"))
}

fn label(s: PaneState) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

fn default_true() -> bool {
    true
}
