use super::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Fixture {
    schema_version: u32,
    #[serde(default)]
    captured: Option<CaptureInfo>,
    context: RuntimeContext,
    operations: Vec<ReplayOperation>,
    rpc: Vec<Record>,
    expected: Expected,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaptureInfo {
    cli_version: String,
    model: String,
    scenario: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplayOperation {
    operation: RuntimeOperation,
    context: Option<RuntimeContext>,
}
#[derive(Deserialize)]
#[serde(
    tag = "direction",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Record {
    Outbound {
        #[serde(default)]
        receipt_ms: Option<f64>,
        operation: usize,
        action: RuntimeAction,
    },
    Inbound {
        #[serde(default)]
        receipt_ms: Option<f64>,
        operation: Option<usize>,
        #[serde(default)]
        reply: Option<RpcReply>,
        #[serde(default)]
        frame: Option<RuntimeFrame>,
        #[serde(default)]
        context: Option<RuntimeContext>,
    },
}
#[derive(Debug, Deserialize, serde::Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ResultEntry {
    operation: usize,
    step: RuntimeStep,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    results: Vec<ResultEntry>,
    events: Vec<AgentRuntimeEvent>,
}

struct FixturePaths {
    dir: PathBuf,
    manifest: PathBuf,
    declaration: PathBuf,
}
fn paths() -> Vec<FixturePaths> {
    let supplied = [
        "CODETERM_RUNTIME_FIXTURES",
        "CODETERM_RUNTIME_MANIFEST",
        "CODETERM_RUNTIME_DECLARATION",
    ]
    .map(std::env::var_os);
    if supplied.iter().any(Option::is_some) {
        let [dir, manifest, declaration] = supplied.map(|value| {
            PathBuf::from(value.expect("all three explicit runtime fixture paths are required"))
        });
        return vec![FixturePaths {
            dir,
            manifest,
            declaration,
        }];
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("plugins");
    let mut directories: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    let discovered: Vec<_> = directories
        .into_iter()
        .filter_map(|bundle| {
            let dir = bundle.join("tests/fixtures/runtime");
            let declaration = bundle.join("tests/runtime-manifest.json");
            let manifest = bundle.join("plugin.json");
            (dir.is_dir() && declaration.is_file() && manifest.is_file()).then_some(FixturePaths {
                dir,
                manifest,
                declaration,
            })
        })
        .collect();
    assert!(
        !discovered.is_empty(),
        "runtime fixture inputs must exist and bundles must be built"
    );
    discovered
}

fn load_provider(manifest_path: &Path, declaration_path: &Path) -> JsProvider {
    let mut raw: Value =
        serde_json::from_str(&std::fs::read_to_string(manifest_path).unwrap()).unwrap();
    let extra: Value =
        serde_json::from_str(&std::fs::read_to_string(declaration_path).unwrap()).unwrap();
    for field in ["agentRuntime", "chatBackend"] {
        raw["capabilities"][field] = extra[field].clone();
    }
    let manifest = PluginManifest::from_json_str(&raw.to_string()).unwrap();
    let bundle = manifest_path.parent().unwrap().join(&manifest.entry);
    let vm = PluginVm::load_with_manifest(
        &manifest.id,
        &bundle,
        serde_json::to_string(&manifest).unwrap(),
    )
    .unwrap();
    JsProvider::new(manifest, Arc::new(vm))
}
fn scenario_paths(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
}
fn fixtures(dir: &Path) -> Vec<(PathBuf, Fixture)> {
    let mut paths = scenario_paths(dir);
    assert_eq!(
        paths.len(),
        8,
        "shared runtime gate requires all eight scenarios"
    );
    let captures = dir.join("captured");
    if captures.exists() {
        paths.extend(scenario_paths(&captures));
    }
    paths
        .into_iter()
        .map(|path| {
            let fixture: Fixture =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut previous = 0.0;
            for record in &fixture.rpc {
                let receipt = match record {
                    Record::Outbound { receipt_ms, .. } | Record::Inbound { receipt_ms, .. } => {
                        receipt_ms
                    }
                };
                if let Some(receipt) = receipt {
                    assert!(receipt.is_finite() && *receipt >= previous);
                    previous = *receipt;
                } else {
                    assert!(
                        fixture.captured.is_none(),
                        "captured RPC record must have receiptMs"
                    );
                }
            }
            (path, fixture)
        })
        .collect()
}

async fn replay(
    provider: &dyn AgentProvider,
    fixture: &Fixture,
) -> (Vec<ResultEntry>, Vec<AgentRuntimeEvent>) {
    assert_eq!(fixture.schema_version, 1);
    if let Some(capture) = &fixture.captured {
        assert!(
            !capture.cli_version.is_empty()
                && !capture.model.is_empty()
                && !capture.scenario.is_empty()
        );
    }
    let mut plans: HashMap<usize, RuntimePlan> = HashMap::new();
    let mut counts: HashMap<usize, usize> = HashMap::new();
    let mut state = Value::Null;
    let mut binding = None;
    let mut results = Vec::new();
    let mut events = Vec::new();
    for record in &fixture.rpc {
        match record {
            Record::Outbound {
                operation, action, ..
            } => {
                let case = fixture
                    .operations
                    .get(*operation)
                    .expect("operation index exists");
                let ctx = case.context.as_ref().unwrap_or(&fixture.context);
                if !plans.contains_key(operation) {
                    assert!(
                        !results
                            .iter()
                            .any(|result: &ResultEntry| result.operation == *operation),
                        "terminal operation must not replay"
                    );
                    plans.insert(
                        *operation,
                        provider.runtime_plan(ctx, &case.operation).await.unwrap(),
                    );
                }
                assert_eq!(&plans[operation].action, action);
                if matches!(action, RuntimeAction::QuestionAnswer { .. }) {
                    plans.remove(operation);
                }
            }
            Record::Inbound {
                operation: Some(index),
                reply: Some(reply),
                frame: None,
                context: None,
                ..
            } => {
                let case = &fixture.operations[*index];
                let ctx = case.context.as_ref().unwrap_or(&fixture.context);
                let plan = plans
                    .remove(index)
                    .expect("reply must have an outstanding plan");
                let count = counts.entry(*index).or_insert(0);
                *count += 1;
                assert!(*count <= MAX_RUNTIME_STEPS);
                let step = provider
                    .runtime_response(ctx, &case.operation, &plan.state, reply)
                    .await
                    .unwrap();
                if let RuntimeStep::Continue { plan, .. } = &step {
                    plans.insert(*index, plan.clone());
                } else {
                    if let RuntimeStep::Complete {
                        events: Some(response_events),
                        ..
                    } = &step
                    {
                        events.extend(response_events.clone());
                    }
                    results.push(ResultEntry {
                        operation: *index,
                        step,
                    });
                }
            }
            Record::Inbound {
                operation: None,
                reply: None,
                frame: Some(frame),
                context,
                ..
            } => {
                let ctx = context.as_ref().unwrap_or(&fixture.context);
                let authority = (
                    ctx.epoch,
                    ctx.tab_id.clone(),
                    ctx.binding_nonce.clone(),
                    ctx.thread_id.clone(),
                );
                if binding.as_ref() != Some(&authority) {
                    state = Value::Null;
                    binding = Some(authority);
                }
                let reduced = provider
                    .runtime_reduce(ctx, &state, std::slice::from_ref(frame))
                    .await
                    .unwrap();
                state = reduced.state;
                events.extend(reduced.events);
            }
            _ => panic!("invalid replay record envelope"),
        }
    }
    assert!(plans.is_empty(), "every operation must terminate");
    (results, events)
}

#[tokio::test]
async fn runtime_shared_provider_fixture_replay() {
    for paths in paths() {
        let provider = load_provider(&paths.manifest, &paths.declaration);
        for (path, fixture) in fixtures(&paths.dir) {
            let (results, events) = replay(&provider, &fixture).await;
            assert_eq!(
                results,
                fixture.expected.results,
                "{} results",
                path.display()
            );
            assert_eq!(events, fixture.expected.events, "{} events", path.display());
            eprintln!(
                "RUNTIME_REPLAY_PASS {} operations={} events={}",
                path.display(),
                results.len(),
                events.len()
            );
        }
    }
}

fn p99(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples[(samples.len() * 99).div_ceil(100).saturating_sub(1)]
}

#[tokio::test]
async fn runtime_quickjs_frame_mapping_benchmark() {
    for paths in paths() {
        benchmark(&paths).await;
    }
}

async fn benchmark(paths: &FixturePaths) {
    let fixtures = fixtures(&paths.dir);
    let (rate, peak) = observed_rates(&fixtures);
    let provider = load_provider(&paths.manifest, &paths.declaration);
    let mut batches = Vec::new();
    for (_, fixture) in &fixtures {
        for record in &fixture.rpc {
            if let Record::Inbound {
                operation: None,
                reply: None,
                frame: Some(frame),
                context,
                ..
            } = record
            {
                batches.push((
                    context.as_ref().unwrap_or(&fixture.context).clone(),
                    frame.clone(),
                ));
            }
        }
    }
    assert!(!batches.is_empty());
    let (ctx, frame) = &batches[0];
    let start = Instant::now();
    provider
        .runtime_reduce(ctx, &Value::Null, std::slice::from_ref(frame))
        .await
        .unwrap();
    let first_call = start.elapsed();
    let mut js_samples = Vec::new();
    let start = Instant::now();
    for _ in 0..20 {
        let mut state = Value::Null;
        let mut authority = None;
        for (ctx, frame) in &batches {
            let current = (ctx.epoch, ctx.binding_nonce.clone(), ctx.thread_id.clone());
            if authority.as_ref() != Some(&current) {
                state = Value::Null;
                authority = Some(current);
            }
            let started = Instant::now();
            let mapped = provider
                .runtime_reduce(ctx, &state, std::slice::from_ref(frame))
                .await
                .unwrap();
            js_samples.push(started.elapsed());
            state = mapped.state;
            std::hint::black_box(mapped.events);
        }
    }
    let js_elapsed = start.elapsed();
    let mut native_samples = Vec::new();
    let start = Instant::now();
    for _ in 0..20 {
        let mut state = Value::Null;
        for (_, frame) in &batches {
            let started = Instant::now();
            let mapped = super::native::native_frame_baseline(&state, std::slice::from_ref(frame));
            native_samples.push(started.elapsed());
            state = mapped.state;
            std::hint::black_box(mapped.events);
        }
    }
    let native_elapsed = start.elapsed();
    let frames = js_samples.len();
    let js_fps = frames as f64 / js_elapsed.as_secs_f64();
    let native_fps = frames as f64 / native_elapsed.as_secs_f64();
    eprintln!("RUNTIME_BENCH_PEAK measured_peak_fps={peak:.1} peak_bar_fps={:.1} quickjs_fps={js_fps:.1} peak_headroom_clear={}",peak*2.0,js_fps>=peak*2.0);

    let js_p99 = p99(&mut js_samples);
    let native_p99 = p99(&mut native_samples);
    eprintln!("RUNTIME_BENCH batch_size=1 frames={frames} first_call_ms={:.3} quickjs_fps={js_fps:.1} quickjs_p99_ms={:.3} native_baseline_fps={native_fps:.1} native_baseline_p99_ms={:.3} measured_stream_fps={rate:.1} headroom=2.0 bar_fps={:.1}",first_call.as_secs_f64()*1000.0,js_p99.as_secs_f64()*1000.0,native_p99.as_secs_f64()*1000.0,rate*2.0);
    assert!(
        js_fps >= rate * 2.0,
        "QuickJS must sustain measured streaming rate with 2x headroom"
    );
}

fn receipt(record: &Record) -> Option<f64> {
    match record {
        Record::Outbound { receipt_ms, .. } | Record::Inbound { receipt_ms, .. } => *receipt_ms,
    }
}
fn observed_rates(fixtures: &[(PathBuf, Fixture)]) -> (f64, f64) {
    let mut sustained = 0.0_f64;
    let mut peak = 0.0_f64;
    for (path, fixture) in fixtures {
        if fixture.captured.is_none() {
            continue;
        }
        let start = receipt(fixture.rpc.first().expect("capture contains RPC records")).unwrap();
        let end = receipt(fixture.rpc.last().unwrap()).unwrap();
        let times: Vec<_> = fixture
            .rpc
            .iter()
            .filter_map(|record| match record {
                Record::Inbound {
                    frame: Some(RuntimeFrame::Notification { .. }),
                    receipt_ms: Some(at),
                    ..
                } => Some(*at),
                _ => None,
            })
            .collect();
        let maximum = |duration: f64| {
            std::iter::once(start)
                .chain(times.iter().copied())
                .filter(|at| at + duration <= end)
                .map(|from| {
                    times
                        .iter()
                        .filter(|at| **at >= from && **at < from + duration)
                        .count() as f64
                        / (duration / 1000.0)
                })
                .reduce(f64::max)
        };
        if let Some(rate) = maximum(10000.0) {
            sustained = sustained.max(rate);
        }
        if let Some(rate) = maximum(1000.0) {
            peak = peak.max(rate);
        }
        eprintln!(
            "RUNTIME_CAPTURE_RATE {} sustained_fps={:?} peak_fps={:?}",
            path.display(),
            maximum(10000.0),
            maximum(1000.0)
        );
    }
    assert!(
        sustained.is_finite() && sustained > 0.0 && peak >= sustained,
        "a real complete ten-second streaming capture is required"
    );
    (sustained, peak)
}
