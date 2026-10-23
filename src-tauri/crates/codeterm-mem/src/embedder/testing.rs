use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use parking_lot::Mutex;

use super::provider::{self, Content, EmbedError, EmbeddingProvider, Modality};
use super::{ActiveEmbedder, Clock, EmbedderEvent};
use crate::store::{MemKind, MemScope, NewMemItem};
use crate::MemManager;

pub(crate) const DEADLINE: Duration = Duration::from_secs(30);
const MODALITIES: &[Modality] = &[Modality::Text];

type Gate = (mpsc::Sender<String>, Arc<Mutex<mpsc::Receiver<()>>>);

/// Deterministic bag-of-words embedder: texts sharing words are close.
pub(crate) struct MockProvider {
    id: String,
    model: String,
    dimension: usize,
    remote: bool,
    down: AtomicBool,
    pub calls: AtomicUsize,
    gate: Mutex<Option<Gate>>,
    failures: Mutex<Vec<(String, EmbedError)>>,
}

impl MockProvider {
    pub fn new(id: &str, model: &str, dimension: usize) -> Arc<Self> {
        Arc::new(Self {
            id: id.into(),
            model: model.into(),
            dimension,
            remote: false,
            down: AtomicBool::new(false),
            calls: AtomicUsize::new(0),
            gate: Mutex::new(None),
            failures: Mutex::new(Vec::new()),
        })
    }

    pub fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }

    /// Every later passage reports its text on the returned receiver and
    /// waits for one release before answering.
    pub fn gated(&self) -> (mpsc::Receiver<String>, mpsc::Sender<()>) {
        let (started_tx, started) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        *self.gate.lock() = Some((started_tx, Arc::new(Mutex::new(release_rx))));
        (started, release)
    }

    /// Every later passage containing `word` fails with `error`.
    pub fn fail_on(&self, word: &str, error: EmbedError) {
        self.failures.lock().push((word.into(), error));
    }

    pub fn ungate(&self) {
        *self.gate.lock() = None;
    }

    pub fn vector(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; self.dimension];
        for word in text.split_whitespace() {
            let hash = word.bytes().fold(1469598103934665603u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            });
            vector[(hash % self.dimension as u64) as usize] += 1.0;
        }
        vector[0] += 0.01;
        let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
        vector.into_iter().map(|v| v / norm).collect()
    }

    fn embed(&self, content: &Content, gated: bool) -> Result<Vec<f32>, EmbedError> {
        let text = provider::supported_text(MODALITIES, content)?.to_string();
        let gate = if gated {
            self.gate.lock().clone()
        } else {
            None
        };
        if let Some((started, release)) = gate {
            let _ = started.send(text.clone());
            release
                .lock()
                .recv_timeout(DEADLINE)
                .map_err(|_| EmbedError::Unavailable("gate closed".into()))?;
        }
        if self.down.load(Ordering::SeqCst) {
            return Err(EmbedError::Unavailable("mock outage".into()));
        }
        if gated {
            if let Some((_, error)) = self
                .failures
                .lock()
                .iter()
                .find(|(word, _)| text.contains(word.as_str()))
            {
                return Err(error.clone());
            }
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.vector(&text))
    }
}

impl EmbeddingProvider for MockProvider {
    fn id(&self) -> &str {
        &self.id
    }
    fn model(&self) -> &str {
        &self.model
    }
    fn dimension(&self) -> usize {
        self.dimension
    }
    fn max_tokens(&self) -> usize {
        512
    }
    fn modalities(&self) -> &[Modality] {
        MODALITIES
    }
    fn remote(&self) -> bool {
        self.remote
    }
    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed(content, true)
    }
    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed(content, false)
    }
}

/// Each reading advances one second from a fixed epoch.
pub(crate) struct StepClock(AtomicI64);

impl StepClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self(AtomicI64::new(0)))
    }

    pub fn at(step: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + step, 0).unwrap()
    }
}

impl Clock for StepClock {
    fn now(&self) -> DateTime<Utc> {
        Self::at(self.0.fetch_add(1, Ordering::SeqCst))
    }
}

pub(crate) struct Fixture {
    pub _dir: tempfile::TempDir,
    pub mem: Arc<MemManager>,
    pub embedder: Arc<ActiveEmbedder>,
    pub events: mpsc::Receiver<EmbedderEvent>,
}

pub(crate) fn fixture(provider: Arc<MockProvider>, clock: Arc<dyn Clock>) -> Fixture {
    fixture_with_docs(provider, clock, false)
}

pub(crate) fn fixture_for(provider: super::provider::SharedProvider) -> Fixture {
    fixture_with_docs(provider, Arc::new(super::SystemClock), false)
}

pub(crate) fn fixture_with_docs(
    provider: super::provider::SharedProvider,
    clock: Arc<dyn Clock>,
    docs: bool,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let embedder = ActiveEmbedder::new(provider);
    let (sender, events) = mpsc::channel();
    let sender = Mutex::new(sender);
    embedder.subscribe(Arc::new(move |event| {
        let _ = sender.lock().send(event.clone());
    }));
    let docs_dir = docs.then(|| dir.path().to_path_buf());
    let mem = MemManager::open_with(dir.path(), embedder.clone(), clock, docs_dir).unwrap();
    Fixture {
        _dir: dir,
        mem,
        embedder,
        events,
    }
}

pub(crate) fn note(title: &str, body: &str) -> NewMemItem {
    NewMemItem {
        kind: MemKind::Note,
        scope: MemScope::Global,
        title: title.into(),
        summary: None,
        area: None,
        tags: vec![],
        body: body.into(),
        source: None,
        importance: 3.0,
        source_path: None,
        pinned: false,
    }
}

pub(crate) fn vector_rows(mem: &MemManager) -> Vec<String> {
    mem.with_conn(|conn| {
        let mut stmt = conn
            .prepare("SELECT mem_id FROM mem_vectors ORDER BY mem_id")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    })
}

pub(crate) fn wait_for(
    events: &mpsc::Receiver<EmbedderEvent>,
    mut matches: impl FnMut(&EmbedderEvent) -> bool,
) -> EmbedderEvent {
    loop {
        let event = events
            .recv_timeout(DEADLINE)
            .expect("expected embedder event never arrived");
        if matches(&event) {
            return event;
        }
    }
}

pub(crate) fn connection() -> rusqlite::Connection {
    crate::embeddings::ensure_extension_registered();
    rusqlite::Connection::open_in_memory().unwrap()
}
