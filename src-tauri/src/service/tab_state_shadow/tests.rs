use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc;

use crate::mem::decision::{DecisionError, DecisionPrimitives};
use crate::service::pty_io::SendBatchItem;
use parking_lot::Mutex;

const TAB: &str = "tab-shadow-1";

#[derive(Clone)]
struct StubHost {
    screen: String,
    reads: Arc<AtomicUsize>,
    writes: Arc<AtomicUsize>,
    models: Arc<DecisionRegistry>,
    tally: Arc<ShadowTally>,
    lines: Arc<Mutex<Vec<String>>>,
}

impl StubHost {
    fn new(screen: &str, models: DecisionRegistry) -> Self {
        Self {
            screen: screen.to_owned(),
            reads: Arc::default(),
            writes: Arc::default(),
            models: Arc::new(models),
            tally: Arc::default(),
            lines: Arc::default(),
        }
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    fn writes(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().clone()
    }
}

impl ShadowHost for StubHost {
    fn transport(&self, _tab_id: &str) -> Result<Box<dyn TabTransport + '_>, String> {
        Ok(Box::new(StubTransport { host: self }))
    }

    fn decision_models(&self) -> Arc<DecisionRegistry> {
        self.models.clone()
    }

    fn tally(&self) -> &ShadowTally {
        &self.tally
    }

    fn record(&self, line: &str) {
        self.lines.lock().push(line.to_owned());
    }
}

struct StubTransport<'a> {
    host: &'a StubHost,
}

impl StubTransport<'_> {
    fn write(&self) -> Result<(), String> {
        self.host.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

impl TabTransport for StubTransport<'_> {
    fn send(&self, _text: &str, _enter: bool) -> Result<(), String> {
        self.write()
    }

    fn send_batch(&self, _batch: Vec<SendBatchItem>) -> Result<(), String> {
        self.write()
    }

    fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        self.write()
    }

    fn kill(&self) -> Result<(), String> {
        self.write()
    }

    fn read_output(
        &self,
        _offset: Option<usize>,
        _max_bytes: Option<usize>,
        _strip_ansi: bool,
        _full: bool,
    ) -> Result<serde_json::Value, String> {
        self.host.reads.fetch_add(1, Ordering::SeqCst);
        Err("the shadow reads the screen, not raw output".into())
    }

    fn read_screen(&self, formatted: bool) -> Result<serde_json::Value, String> {
        self.host.reads.fetch_add(1, Ordering::SeqCst);
        Ok(serde_json::json!({ "text": self.host.screen, "formatted": formatted }))
    }
}

struct StubModel {
    primitives: DecisionPrimitives,
    answer: Result<DecisionAnswer, DecisionError>,
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Option<DecisionRequest>>>,
    gate: Option<Mutex<mpsc::Receiver<()>>>,
}

impl StubModel {
    fn answering(answer: Result<DecisionAnswer, DecisionError>) -> Self {
        Self {
            primitives: DecisionPrimitives {
                choice: true,
                noul: false,
                score: false,
            },
            answer,
            calls: Arc::default(),
            seen: Arc::default(),
            gate: None,
        }
    }
}

impl DecisionModel for StubModel {
    fn id(&self) -> &str {
        "stub"
    }

    fn primitives(&self) -> DecisionPrimitives {
        self.primitives
    }

    fn decide(
        &self,
        request: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock() = Some(request.clone());
        if let Some(gate) = &self.gate {
            gate.lock()
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| DecisionError::Deadline)?;
        }
        self.answer.clone()
    }
}

fn choice(label: &str, confidence: Option<f64>) -> DecisionAnswer {
    DecisionAnswer::Choice {
        choice: label.to_owned(),
        probabilities: BTreeMap::from([(label.to_owned(), 0.91)]),
        confidence,
    }
}

fn registry(entries: Vec<(&str, StubModel)>) -> DecisionRegistry {
    let registry = DecisionRegistry::new();
    for (id, model) in entries {
        registry.register(id, Arc::new(model));
    }
    registry
}

fn enabled() -> TabStateShadowConfig {
    TabStateShadowConfig { enabled: true }
}

fn run(host: &StubHost, structural: StartupNudge) -> Option<ShadowVerdict> {
    spawn_observer(host, &enabled(), TAB, structural)
        .expect("an enabled shadow starts its observer")
        .join()
        .expect("observer thread completes")
}

#[test]
fn tab_state_shadow_disabled_makes_no_read_or_call() {
    let model = StubModel::answering(Ok(choice(READY_IDLE, Some(0.99))));
    let calls = model.calls.clone();
    let host = StubHost::new("❯ ", registry(vec![("stub", model)]));

    let default_config = crate::config::schema::AppConfig::default()
        .agent
        .tab_state_shadow;
    assert!(!default_config.enabled, "the shadow ships default-off");
    for structural in [StartupNudge::Deliver, StartupNudge::BlockedOnPrompt] {
        assert!(spawn_observer(&host, &default_config, TAB, structural).is_none());
    }

    assert_eq!(host.reads(), 0, "a disabled shadow reads no screen");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "a disabled shadow calls no model"
    );
    assert_eq!(host.writes(), 0);
    assert!(host.lines().is_empty());
}

#[test]
fn tab_state_shadow_logs_structural_and_model_verdict() {
    let screen: String = (1..=50).map(|n| format!("boot line {n}\r\n")).collect();
    let (release, gate) = mpsc::channel();
    let mut model = StubModel::answering(Ok(choice(STILL_STARTING, Some(0.93))));
    model.gate = Some(Mutex::new(gate));
    let calls = model.calls.clone();
    let seen = model.seen.clone();
    let host = StubHost::new(&screen, registry(vec![("stub", model)]));

    let handle = spawn_observer(&host, &enabled(), TAB, StartupNudge::UnboundSession)
        .expect("an enabled shadow starts its observer");
    assert!(
        host.lines().is_empty(),
        "the hook returns before the model answers"
    );
    release.send(()).expect("observer is waiting on the gate");
    let verdict = handle.join().expect("observer thread completes");

    assert_eq!(
        verdict,
        Some(ShadowVerdict {
            label: STILL_STARTING.to_owned(),
            confidence: 0.93,
            agrees: true,
        })
    );
    let lines = host.lines();
    assert_eq!(
        lines.len(),
        1,
        "exactly one shadow line per spawn-health event"
    );
    let line = &lines[0];
    assert!(line.contains(&format!("tab={TAB} ")));
    assert!(line.contains(&format!("{:?}", StartupNudge::UnboundSession.reason())));
    assert!(line.contains(&format!("model={STILL_STARTING} ")));
    assert!(line.contains("agree=1 disagree=0 uncertain=0"));

    assert_eq!((host.reads(), calls.load(Ordering::SeqCst)), (1, 1));
    assert_eq!(host.writes(), 0, "the shadow never writes into the tab");
    let request = seen.lock().clone().expect("the model saw the request");
    let expected_tail: Vec<String> = (11..=50).map(|n| format!("boot line {n}")).collect();
    assert_eq!(
        request.state,
        serde_json::Value::String(expected_tail.join("\n"))
    );
    let DecisionQuestion::Choice { options } = request.question else {
        panic!("the shadow asks a choice question");
    };
    let labels: Vec<&str> = options.keys().map(String::as_str).collect();
    let mut expected: Vec<&str> = CRITERIA.iter().map(|(label, _)| *label).collect();
    expected.sort_unstable();
    assert_eq!(labels, expected);

    let disagreeing = StubModel::answering(Ok(choice(ERROR_LOOP, None)));
    let host = StubHost {
        models: Arc::new(registry(vec![("stub", disagreeing)])),
        ..host
    };
    let verdict = run(&host, StartupNudge::Deliver).expect("a verdict");
    assert_eq!((verdict.confidence, verdict.agrees), (0.91, false));
    assert!(host.lines()[1].contains("agree=1 disagree=1 uncertain=0"));
}

#[test]
fn tab_state_shadow_model_error_changes_nothing() {
    let failures = [
        Err(DecisionError::Deadline),
        Err(DecisionError::Http(500)),
        Err(DecisionError::Unavailable("token_missing".into())),
        Ok(choice("not_a_tab_state", Some(0.99))),
        Ok(DecisionAnswer::Noul { p: 0.9 }),
    ];
    for answer in failures {
        let model = StubModel::answering(answer.clone());
        let calls = model.calls.clone();
        let host = StubHost::new("❯ ", registry(vec![("stub", model)]));

        assert_eq!(run(&host, StartupNudge::Deliver), None, "{answer:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(host.lines().is_empty(), "{answer:?} emits no shadow line");
        assert_eq!(host.writes(), 0);
        assert_eq!(
            host.tally().count(&ShadowVerdict {
                label: READY_IDLE.into(),
                confidence: 1.0,
                agrees: true,
            }),
            (1, 0, 0),
            "{answer:?} leaves the tally untouched"
        );
    }
}

#[test]
fn tab_state_shadow_requires_choice_primitive() {
    let mut scoring = StubModel::answering(Ok(choice(READY_IDLE, Some(0.99))));
    scoring.primitives = DecisionPrimitives {
        choice: false,
        noul: true,
        score: true,
    };
    let scoring_calls = scoring.calls.clone();
    let host = StubHost::new("❯ ", registry(vec![("a-scoring", scoring)]));
    assert_eq!(run(&host, StartupNudge::Deliver), None);
    assert_eq!(scoring_calls.load(Ordering::SeqCst), 0);
    assert_eq!(host.reads(), 0, "no choice model means no screen read");

    let empty = StubHost::new("❯ ", DecisionRegistry::new());
    assert_eq!(run(&empty, StartupNudge::Deliver), None);
    assert_eq!(empty.reads(), 0);

    let mut scoring = StubModel::answering(Ok(choice(READY_IDLE, Some(0.99))));
    scoring.primitives.choice = false;
    let scoring_calls = scoring.calls.clone();
    let chooser = StubModel::answering(Ok(choice(READY_IDLE, Some(0.99))));
    let chooser_calls = chooser.calls.clone();
    let host = StubHost::new(
        "❯ ",
        registry(vec![("a-scoring", scoring), ("z-choice", chooser)]),
    );
    assert!(run(&host, StartupNudge::Deliver).is_some());
    assert_eq!(scoring_calls.load(Ordering::SeqCst), 0);
    assert_eq!(chooser_calls.load(Ordering::SeqCst), 1);
}
