use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::{Mutex, RwLock};

use super::provider::{Content, EmbedError, EmbedderIdentity, SharedProvider};
use super::switch::ReembedProgress;
use super::StampedVector;

#[derive(Debug, Clone, PartialEq)]
pub enum EmbedderEvent {
    Recovered,
    Unavailable(String),
    Switched(EmbedderIdentity),
    Progress(ReembedProgress),
}

mod query;

pub type Observer = Arc<dyn Fn(&EmbedderEvent) + Send + Sync>;

/// The single active embedder for all of mem. Swapping the provider is the
/// only way to change models; every vector it returns carries its identity.
pub struct ActiveEmbedder {
    provider: RwLock<SharedProvider>,
    available: AtomicBool,
    probing: Arc<AtomicBool>,
    queries: query::QueryCache,
    observers: Mutex<Vec<Observer>>,
}

impl ActiveEmbedder {
    pub fn new(provider: SharedProvider) -> Arc<Self> {
        Arc::new(Self {
            provider: RwLock::new(provider),
            available: AtomicBool::new(true),
            probing: Arc::new(AtomicBool::new(false)),
            queries: query::QueryCache::default(),
            observers: Mutex::new(Vec::new()),
        })
    }

    pub fn global() -> Arc<Self> {
        static GLOBAL: OnceLock<Arc<ActiveEmbedder>> = OnceLock::new();
        GLOBAL
            .get_or_init(|| Self::new(super::catalog::default_provider()))
            .clone()
    }

    pub fn provider(&self) -> SharedProvider {
        self.provider.read().clone()
    }

    pub fn identity(&self) -> EmbedderIdentity {
        self.provider().identity()
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    pub fn replace(&self, provider: SharedProvider) {
        let identity = self.replace_silent(provider);
        self.emit(&EmbedderEvent::Switched(identity));
    }

    pub fn replace_silent(&self, provider: SharedProvider) -> EmbedderIdentity {
        let identity = provider.identity();
        let mut current = self.provider.write();
        self.queries.invalidate();
        *current = provider;
        self.available.store(true, Ordering::SeqCst);
        identity
    }

    pub fn subscribe(&self, observer: Observer) {
        self.observers.lock().push(observer);
    }

    pub fn emit(&self, event: &EmbedderEvent) {
        let observers = self.observers.lock().clone();
        for observer in observers {
            observer(event);
        }
    }

    pub fn passage(&self, text: &str) -> Result<StampedVector, EmbedError> {
        let provider = self.provider();
        let content = Content::Text(crate::embeddings::preprocess(text));
        let result = guard(&provider, &content).and_then(|()| provider.embed_passage(&content));
        self.observe(&provider, &result);
        stamp(&provider, result)
    }

    pub fn passages(&self, texts: &[String]) -> Vec<Result<StampedVector, EmbedError>> {
        let provider = self.provider();
        let contents: Vec<Content> = texts
            .iter()
            .map(|text| Content::Text(crate::embeddings::preprocess(text)))
            .collect();
        let mut results: Vec<Option<Result<Vec<f32>, EmbedError>>> = contents
            .iter()
            .map(|content| guard(&provider, content).err().map(Err))
            .collect();
        let allowed: Vec<Content> = contents
            .iter()
            .zip(&results)
            .filter(|(_, blocked)| blocked.is_none())
            .map(|(content, _)| content.clone())
            .collect();
        let mut embedded = provider.embed_passages(&allowed).into_iter();
        for slot in results.iter_mut().filter(|slot| slot.is_none()) {
            *slot = Some(embedded.next().unwrap_or_else(|| {
                Err(EmbedError::Refused(
                    "provider returned too few vectors".into(),
                ))
            }));
        }
        results
            .into_iter()
            .flatten()
            .map(|result| {
                self.observe(&provider, &result);
                stamp(&provider, result)
            })
            .collect()
    }

    /// The query vector, or `None` for FTS-only search. While the provider is
    /// unavailable search never waits on it; a single background probe with
    /// this query detects recovery.
    pub fn query(self: &Arc<Self>, text: &str) -> Option<StampedVector> {
        super::recall_priority::record(self.bounded_query(text))
    }

    fn bounded_query(self: &Arc<Self>, text: &str) -> Option<StampedVector> {
        let started = std::time::Instant::now();
        let budget = super::recall_priority::remaining();
        let content = Content::Text(crate::embeddings::preprocess(text));
        let current = self.provider.read();
        let provider = current.clone();
        if guard(&provider, &content).is_err() {
            return None;
        }
        if !self.is_available() {
            drop(current);
            if !budget.is_zero() {
                self.probe(provider, content);
            }
            return None;
        }
        let Content::Text(text) = &content else {
            unreachable!()
        };
        let admission = self.queries.admit(text, budget);
        drop(current);
        let admission = admission?;
        let (key, flight, start) = match admission {
            query::Admission::Cached(vector) => return Some(vector),
            query::Admission::Pending { key, flight, start } => (key, flight, start),
        };
        if start {
            let embedder = self.clone();
            let completed = flight.clone();
            let spawned = std::thread::Builder::new()
                .name("mem-recall-query".into())
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        provider.embed_query(&content)
                    }))
                    .unwrap_or_else(|_| {
                        Err(EmbedError::Unavailable("embedding query panicked".into()))
                    });
                    embedder.observe(&provider, &result);
                    embedder
                        .queries
                        .finish(key, &completed, stamp(&provider, result));
                });
            if let Err(error) = spawned {
                self.queries.finish(
                    key,
                    &flight,
                    Err(EmbedError::Unavailable(error.to_string())),
                );
            }
        }
        flight.wait(budget.saturating_sub(started.elapsed()))
    }

    fn probe(self: &Arc<Self>, provider: SharedProvider, content: Content) {
        if self.probing.swap(true, Ordering::SeqCst) {
            return;
        }
        let embedder = self.clone();
        let spawned = std::thread::Builder::new()
            .name("mem-embed-probe".into())
            .spawn(move || {
                let result = provider.embed_query(&content);
                embedder.probing.store(false, Ordering::SeqCst);
                embedder.observe(&provider, &result);
            });
        if spawned.is_err() {
            self.probing.store(false, Ordering::SeqCst);
        }
    }

    fn observe<T>(&self, provider: &SharedProvider, result: &Result<T, EmbedError>) {
        let current = self.provider.read();
        if !Arc::ptr_eq(&current, provider) {
            return;
        }
        let event = match result {
            Ok(_) if !self.available.swap(true, Ordering::SeqCst) => Some(EmbedderEvent::Recovered),
            Err(EmbedError::Unavailable(reason) | EmbedError::NotBuilt(reason))
                if self.available.swap(false, Ordering::SeqCst) =>
            {
                Some(EmbedderEvent::Unavailable(reason.clone()))
            }
            _ => None,
        };
        drop(current);
        match &event {
            Some(EmbedderEvent::Recovered) => log::info!(
                "[mem.embedder] provider available again: {}",
                provider.identity()
            ),
            Some(EmbedderEvent::Unavailable(reason)) => log::warn!(
                "[mem.embedder] provider unavailable: {}: {reason}",
                provider.identity()
            ),
            _ => {}
        }
        if let Some(event) = event {
            self.emit(&event);
        }
    }
}

/// Remote providers never receive text the credential filter flags.
fn guard(provider: &SharedProvider, content: &Content) -> Result<(), EmbedError> {
    match content {
        Content::Text(text)
            if provider.remote()
                && codeterm_blueprint_types::blueprint_memory::contains_credentials(text) =>
        {
            Err(EmbedError::Refused(
                "credential filter blocked text bound for a remote embedder".into(),
            ))
        }
        _ => Ok(()),
    }
}

fn stamp(
    provider: &SharedProvider,
    result: Result<Vec<f32>, EmbedError>,
) -> Result<StampedVector, EmbedError> {
    result.map(|vector| StampedVector {
        identity: provider.identity(),
        vector,
    })
}
