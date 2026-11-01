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

pub type Observer = Arc<dyn Fn(&EmbedderEvent) + Send + Sync>;

/// The single active embedder for all of mem. Swapping the provider is the
/// only way to change models; every vector it returns carries its identity.
pub struct ActiveEmbedder {
    provider: RwLock<SharedProvider>,
    available: AtomicBool,
    probing: Arc<AtomicBool>,
    querying: Arc<AtomicBool>,
    observers: Mutex<Vec<Observer>>,
}

impl ActiveEmbedder {
    pub fn new(provider: SharedProvider) -> Arc<Self> {
        Arc::new(Self {
            provider: RwLock::new(provider),
            available: AtomicBool::new(true),
            probing: Arc::new(AtomicBool::new(false)),
            querying: Arc::new(AtomicBool::new(false)),
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
        *self.provider.write() = provider;
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
        self.observe(&result);
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
                self.observe(&result);
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
        let budget = super::recall_priority::remaining();
        if budget.is_zero() {
            return None;
        }
        let provider = self.provider();
        let content = Content::Text(crate::embeddings::preprocess(text));
        if guard(&provider, &content).is_err() {
            return None;
        }
        if !self.is_available() {
            self.probe(provider, content);
            return None;
        }
        if self.querying.swap(true, Ordering::SeqCst) {
            return None;
        }
        let (reply, response) = std::sync::mpsc::sync_channel(1);
        let embedder = self.clone();
        let spawned = std::thread::Builder::new()
            .name("mem-recall-query".into())
            .spawn(move || {
                struct Release(Arc<AtomicBool>);
                impl Drop for Release {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::SeqCst);
                    }
                }
                let release = Release(embedder.querying.clone());
                let result = provider.embed_query(&content);
                embedder.observe(&result);
                drop(release);
                let _ = reply.send(stamp(&provider, result).ok());
            });
        if spawned.is_err() {
            self.querying.store(false, Ordering::SeqCst);
            return None;
        }
        super::recall_priority::wait(budget, |budget| response.recv_timeout(budget).ok()).flatten()
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
                embedder.observe(&result);
            });
        if spawned.is_err() {
            self.probing.store(false, Ordering::SeqCst);
        }
    }

    fn observe<T>(&self, result: &Result<T, EmbedError>) {
        match result {
            Ok(_) => {
                if !self.available.swap(true, Ordering::SeqCst) {
                    log::info!(
                        "[mem.embedder] provider available again: {}",
                        self.identity()
                    );
                    self.emit(&EmbedderEvent::Recovered);
                }
            }
            Err(EmbedError::Unavailable(reason) | EmbedError::NotBuilt(reason)) => {
                if self.available.swap(false, Ordering::SeqCst) {
                    log::warn!(
                        "[mem.embedder] provider unavailable: {}: {reason}",
                        self.identity()
                    );
                    self.emit(&EmbedderEvent::Unavailable(reason.clone()));
                }
            }
            Err(_) => {}
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
