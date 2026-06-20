use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use codeterm_lib::agent::offline_state_classifier::{
    classify_timeline, CastEvent, CastTimelineInput, ClassifiedSegment, QuestionSubtype,
    TimelineState, TICK_MS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug)]
enum Event {
    Output { now_ms: u64, bytes: Vec<u8> },
    Resize { now_ms: u64, rows: u16, cols: u16 },
}

#[derive(Clone, Debug)]
struct Cast {
    width: u16,
    height: u16,
    events: Vec<Event>,
}

impl From<&Cast> for CastTimelineInput {
    fn from(value: &Cast) -> Self {
        Self {
            width: value.width,
            height: value.height,
            events: value
                .events
                .iter()
                .map(|event| match event {
                    Event::Output { now_ms, bytes } => CastEvent::Output {
                        now_ms: *now_ms,
                        bytes: bytes.clone(),
                    },
                    Event::Resize { now_ms, rows, cols } => CastEvent::Resize {
                        now_ms: *now_ms,
                        rows: *rows,
                        cols: *cols,
                    },
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct StateTimeline {
    #[serde(default)]
    steps: Vec<StateStep>,
    segments: Vec<LabeledSegment>,
}

#[derive(Clone, Debug, Deserialize)]
struct StateStep {
    at_ms: u64,
    action: String,
    expect: String,
}

#[derive(Clone, Debug, Deserialize)]
struct LabeledSegment {
    from_ms: u64,
    to_ms: u64,
    state: TimelineState,
    #[serde(default)]
    subtype: Option<QuestionSubtype>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StateCalibrationSummary {
    pub dumps: usize,
    pub providers: BTreeMap<String, ProviderScore>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ProviderScore {
    pub dumps: usize,
    pub precision: BTreeMap<String, f64>,
    pub recall: BTreeMap<String, f64>,
    pub mean_transition_latency_ms: Option<f64>,
    pub subtype_confusion: BTreeMap<String, BTreeMap<String, u64>>,
}

#[derive(Clone, Debug, Default)]
struct Counts {
    truth: u64,
    predicted: u64,
    hit: u64,
}

#[derive(Clone, Debug, Default)]
struct Accumulator {
    dumps: usize,
    counts: BTreeMap<String, Counts>,
    latencies: Vec<i64>,
    subtype_confusion: BTreeMap<String, BTreeMap<String, u64>>,
}

pub fn state_dumps_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../testbench/state-dumps"
    ))
}

pub fn run_state_calibration(root: &Path) -> StateCalibrationSummary {
    let dumps = discover_state_dumps(root);
    println!("\n=== state calibration ===");
    println!(
        "{:<46} {:<10} {:<22} {:<22} latency",
        "dump", "provider", "precision", "recall"
    );
    println!("{}", "-".repeat(116));

    let mut by_provider = BTreeMap::<String, Accumulator>::new();
    for dir in dumps {
        let provider = dir
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".into());
        let case = dir
            .strip_prefix(root)
            .unwrap_or(&dir)
            .to_string_lossy()
            .replace('\\', "/");
        let timeline = parse_timeline(&dir.join("timeline.json"));
        let cast = parse_cast(&dir.join("session.cast"));
        let predicted = classify_timeline(&CastTimelineInput::from(&cast));
        let report = score_dump(&timeline, &predicted);
        print_dump_score(&case, &provider, &timeline, &report);
        by_provider.entry(provider).or_default().merge(report);
    }

    let providers = by_provider
        .into_iter()
        .map(|(provider, acc)| (provider, acc.into_score()))
        .collect::<BTreeMap<_, _>>();
    let summary = StateCalibrationSummary {
        dumps: providers.values().map(|score| score.dumps).sum(),
        providers,
    };
    println!(
        "\nstate calibration aggregate:\n{}",
        serde_json::to_string_pretty(&summary).unwrap()
    );
    summary
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

fn parse_timeline(path: &Path) -> StateTimeline {
    serde_json::from_str(
        &std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
    )
    .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn score_dump(timeline: &StateTimeline, predicted: &[ClassifiedSegment]) -> Accumulator {
    let start = timeline
        .segments
        .first()
        .map(|s| s.from_ms)
        .unwrap_or_default();
    let end = timeline
        .segments
        .last()
        .map(|s| s.to_ms)
        .unwrap_or_default();
    let mut acc = Accumulator {
        dumps: 1,
        ..Accumulator::default()
    };
    let mut tick = start;
    while tick < end {
        let truth = timeline_state_at(&timeline.segments, tick);
        let pred = predicted_state_at(predicted, tick);
        if let Some(truth) = truth {
            let truth_label = state_label(truth.state);
            acc.counts.entry(truth_label).or_default().truth += TICK_MS;
            if truth.state == TimelineState::Question {
                add_subtype_confusion(&mut acc, truth, pred);
            }
            if let Some(pred) = pred {
                let pred_label = state_label(pred.state);
                acc.counts.entry(pred_label.clone()).or_default().predicted += TICK_MS;
                if pred.state == truth.state {
                    acc.counts.entry(pred_label).or_default().hit += TICK_MS;
                }
            }
        } else if let Some(pred) = pred {
            let pred_label = state_label(pred.state);
            acc.counts.entry(pred_label).or_default().predicted += TICK_MS;
        }
        tick += TICK_MS;
    }
    for boundary in labeled_boundaries(&timeline.segments) {
        if let Some(predicted_boundary) = nearest_predicted_boundary(predicted, boundary) {
            acc.latencies
                .push(predicted_boundary as i64 - boundary as i64);
        }
    }
    acc
}

fn add_subtype_confusion(
    acc: &mut Accumulator,
    truth: &LabeledSegment,
    pred: Option<&ClassifiedSegment>,
) {
    let predicted_subtype = pred
        .and_then(|p| p.subtype)
        .map(subtype_label)
        .unwrap_or_else(|| "none".into());
    let truth_subtype = truth
        .subtype
        .map(subtype_label)
        .unwrap_or_else(|| "unknown".into());
    *acc.subtype_confusion
        .entry(truth_subtype)
        .or_default()
        .entry(predicted_subtype)
        .or_default() += TICK_MS;
}

fn timeline_state_at(segments: &[LabeledSegment], tick: u64) -> Option<&LabeledSegment> {
    segments
        .iter()
        .find(|segment| tick >= segment.from_ms && tick < segment.to_ms)
}

fn predicted_state_at(segments: &[ClassifiedSegment], tick: u64) -> Option<&ClassifiedSegment> {
    segments
        .iter()
        .find(|segment| tick >= segment.from_ms && tick < segment.to_ms)
}

fn labeled_boundaries(segments: &[LabeledSegment]) -> Vec<u64> {
    segments
        .windows(2)
        .filter_map(|pair| {
            let a = &pair[0];
            let b = &pair[1];
            (a.state != b.state || a.subtype != b.subtype).then_some(b.from_ms)
        })
        .collect()
}

fn nearest_predicted_boundary(segments: &[ClassifiedSegment], boundary: u64) -> Option<u64> {
    segments
        .iter()
        .skip(1)
        .map(|segment| segment.from_ms)
        .min_by_key(|predicted| predicted.abs_diff(boundary))
}

fn print_dump_score(case: &str, provider: &str, timeline: &StateTimeline, report: &Accumulator) {
    let step_summary = timeline
        .steps
        .iter()
        .map(|s| format!("{}ms:{}=>{}", s.at_ms, s.action, s.expect))
        .collect::<Vec<_>>()
        .join("; ");
    println!(
        "{:<46} {:<10} {:<22} {:<22} {}",
        case,
        provider,
        compact_rates(&report.precision()),
        compact_rates(&report.recall()),
        report
            .mean_latency()
            .map(|latency| format!("{latency:.0}ms"))
            .unwrap_or_else(|| "n/a".into())
    );
    if !step_summary.is_empty() {
        println!("  steps: {step_summary}");
    }
}

fn compact_rates(rates: &BTreeMap<String, f64>) -> String {
    rates
        .iter()
        .map(|(state, value)| format!("{state}:{value:.2}"))
        .collect::<Vec<_>>()
        .join(" ")
}

impl Accumulator {
    fn merge(&mut self, other: Accumulator) {
        self.dumps += other.dumps;
        for (state, counts) in other.counts {
            let target = self.counts.entry(state).or_default();
            target.truth += counts.truth;
            target.predicted += counts.predicted;
            target.hit += counts.hit;
        }
        self.latencies.extend(other.latencies);
        for (truth, preds) in other.subtype_confusion {
            let target = self.subtype_confusion.entry(truth).or_default();
            for (pred, count) in preds {
                *target.entry(pred).or_default() += count;
            }
        }
    }

    fn into_score(self) -> ProviderScore {
        ProviderScore {
            dumps: self.dumps,
            precision: self.precision(),
            recall: self.recall(),
            mean_transition_latency_ms: self.mean_latency(),
            subtype_confusion: self.subtype_confusion,
        }
    }

    fn precision(&self) -> BTreeMap<String, f64> {
        self.counts
            .iter()
            .map(|(state, counts)| {
                let value = if counts.predicted == 0 {
                    0.0
                } else {
                    counts.hit as f64 / counts.predicted as f64
                };
                (state.clone(), value)
            })
            .collect()
    }

    fn recall(&self) -> BTreeMap<String, f64> {
        self.counts
            .iter()
            .map(|(state, counts)| {
                let value = if counts.truth == 0 {
                    0.0
                } else {
                    counts.hit as f64 / counts.truth as f64
                };
                (state.clone(), value)
            })
            .collect()
    }

    fn mean_latency(&self) -> Option<f64> {
        (!self.latencies.is_empty()).then(|| {
            self.latencies.iter().copied().sum::<i64>() as f64 / self.latencies.len() as f64
        })
    }
}

fn state_label(state: TimelineState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

fn subtype_label(subtype: QuestionSubtype) -> String {
    serde_json::to_value(subtype)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}
