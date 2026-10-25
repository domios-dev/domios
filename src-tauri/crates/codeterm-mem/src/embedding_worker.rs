use crate::embedder::local::LocalModel;
use crate::embedding_model::EmbeddingModel;
use parking_lot::Mutex;
use std::cell::Cell;
use std::sync::{mpsc, Arc, OnceLock};
use std::time::Duration;

const QUEUE_CAPACITY: usize = 64;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const QUERY_BURST: usize = 8;
type Reply = mpsc::SyncSender<Result<Vec<f32>, String>>;

struct Request {
    model: &'static LocalModel,
    text: String,
    reply: Reply,
    caller: String,
}

struct Worker {
    bulk: mpsc::SyncSender<Request>,
    query: mpsc::SyncSender<Request>,
    wake: mpsc::SyncSender<()>,
    residency: Arc<Mutex<Option<Vec<String>>>>,
}

struct Receivers {
    queries: Cell<usize>,
    bulk: mpsc::Receiver<Request>,
    query: mpsc::Receiver<Request>,
    wake: mpsc::Receiver<()>,
    residency: Arc<Mutex<Option<Vec<String>>>>,
    #[cfg(test)]
    waiting: Option<mpsc::Sender<()>>,
}

fn channels() -> (Worker, Receivers) {
    let (bulk, bulk_rx) = mpsc::sync_channel(QUEUE_CAPACITY);
    let (query, query_rx) = mpsc::sync_channel(QUEUE_CAPACITY);
    let (wake, wake_rx) = mpsc::sync_channel(1);
    let residency = Arc::new(Mutex::new(None));
    (
        Worker {
            bulk,
            query,
            wake,
            residency: residency.clone(),
        },
        Receivers {
            queries: Cell::new(0),
            bulk: bulk_rx,
            query: query_rx,
            wake: wake_rx,
            residency,
            #[cfg(test)]
            waiting: None,
        },
    )
}

impl Worker {
    fn start() -> Result<Self, String> {
        let (worker, receivers) = channels();
        std::thread::Builder::new()
            .name("mem-embed".into())
            .spawn(move || {
                run(
                    receivers,
                    cpu_pool,
                    EmbeddingModel::load,
                    EmbeddingModel::embed,
                )
            })
            .map_err(|e| format!("start embedding worker: {e}"))?;
        Ok(worker)
    }

    fn embed(
        &self,
        model: &'static LocalModel,
        text: &str,
        query: bool,
    ) -> Result<Vec<f32>, String> {
        let (reply, response) = mpsc::sync_channel(1);
        let request = Request {
            model,
            text: text.into(),
            reply,
            caller: format!(
                "{}:{}",
                std::thread::current().name().unwrap_or("unnamed"),
                if query { "query" } else { "passage" }
            ),
        };
        if query {
            self.query
                .try_send(request)
                .map_err(|e| format!("embedding query queue unavailable: {e}"))?;
        } else {
            self.bulk
                .try_send(request)
                .map_err(|e| format!("embedding busy: {e}"))?;
        }
        let _ = self.wake.try_send(());
        response
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|e| format!("embedding response unavailable: {e}"))?
    }
}

impl Receivers {
    fn try_next(&self) -> Option<Request> {
        if self.queries.get() >= QUERY_BURST {
            if let Ok(request) = self.bulk.try_recv() {
                self.queries.set(0);
                return Some(request);
            }
        }
        if let Ok(request) = self.query.try_recv() {
            self.queries.set(self.queries.get().saturating_add(1));
            return Some(request);
        }
        let request = self.bulk.try_recv().ok();
        if request.is_some() {
            self.queries.set(0);
        }
        request
    }

    fn next_event(&self) -> Result<Option<Request>, mpsc::RecvError> {
        let woke = self.wake.try_recv().is_ok();
        if let Some(request) = self.try_next() {
            return Ok(Some(request));
        }
        self.queries.set(0);
        if woke {
            return Ok(None);
        }
        #[cfg(test)]
        if let Some(waiting) = &self.waiting {
            let _ = waiting.send(());
        }
        self.wake.recv()?;
        Ok(self.try_next())
    }

    #[cfg(test)]
    fn next(&self) -> Result<Request, mpsc::RecvError> {
        loop {
            if let Some(request) = self.next_event()? {
                return Ok(request);
            }
        }
    }
}

static WORKER: OnceLock<Result<Worker, String>> = OnceLock::new();

fn worker() -> Result<&'static Worker, String> {
    WORKER
        .get_or_init(Worker::start)
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn retain_models(models: Vec<String>) {
    if let Ok(worker) = worker() {
        let mut residency = worker.residency.lock();
        if residency.as_ref() == Some(&models) {
            return;
        }
        *residency = Some(models);
        drop(residency);
        let _ = worker.wake.try_send(());
    }
}

#[derive(Default)]
struct ModelCache<M> {
    entries: Vec<(&'static LocalModel, M)>,
}

impl<M> ModelCache<M> {
    fn retain(&mut self, models: &Option<Vec<String>>) {
        if let Some(models) = models {
            self.entries
                .retain(|(descriptor, _)| models.iter().any(|id| id == descriptor.model));
        }
    }
}

pub(crate) fn embed(
    model: &'static LocalModel,
    text: &str,
    query: bool,
) -> Result<Vec<f32>, String> {
    worker()?.embed(model, text, query)
}

fn cpu_pool() -> rayon::ThreadPoolBuilder {
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .thread_name(|index| format!("mem-embed-cpu-{index}"))
}

fn run<M: Sync>(
    receivers: Receivers,
    pool: impl Fn() -> rayon::ThreadPoolBuilder,
    load: impl Fn(&'static LocalModel) -> Result<M, String>,
    forward: impl Fn(&M, &str) -> Result<Vec<f32>, String> + Sync,
) {
    let mut model = ModelCache {
        entries: Vec::new(),
    };
    loop {
        model.retain(&receivers.residency.lock());
        let Ok(request) = receivers.next_event() else {
            break;
        };
        model.retain(&receivers.residency.lock());
        let mut pending = request;
        if pending.is_none() {
            continue;
        }
        // Scoped workers are joined before the coordinator can block on its channel.
        let result = pool().build_scoped(
            |thread| thread.run(),
            |pool| {
                while let Some(request) = pending.take() {
                    process(
                        &mut model,
                        &receivers.residency,
                        request,
                        &load,
                        |model, text| pool.install(|| forward(model, text)),
                    );
                    pending = receivers.try_next();
                }
            },
        );
        if let Err(error) = result {
            if let Some(request) = pending {
                let _ = request
                    .reply
                    .send(Err(format!("start embedding CPU pool: {error}")));
            }
        }
    }
}

fn process<M>(
    model: &mut ModelCache<M>,
    residency: &Mutex<Option<Vec<String>>>,
    request: Request,
    load: &impl Fn(&'static LocalModel) -> Result<M, String>,
    forward: impl FnOnce(&M, &str) -> Result<Vec<f32>, String>,
) {
    let policy = residency.lock().clone();
    model.retain(&policy);
    if policy
        .as_ref()
        .is_some_and(|ids| !ids.iter().any(|id| id == request.model.model))
    {
        let _ = request.reply.send(Err("embedding model retired".into()));
        return;
    }
    if !model
        .entries
        .iter()
        .any(|(loaded, _)| loaded.model == request.model.model)
    {
        let capacity = policy.as_ref().map_or(1, |ids| ids.len().clamp(1, 2));
        if model.entries.len() >= capacity {
            model.entries.remove(0);
        }
        match load(request.model) {
            Ok(loaded) => {
                model.entries.push((request.model, loaded));
                log::debug!(
                    "[embed] loaded candle model {} pid={} caller={} resident={}",
                    request.model.model,
                    std::process::id(),
                    request.caller,
                    model.entries.len()
                );
            }
            Err(error) => {
                log::warn!("[embed] model unavailable: {error}");
                let _ = request.reply.send(Err(error));
                return;
            }
        }
    }
    let result = model
        .entries
        .iter()
        .find(|(loaded, _)| loaded.model == request.model.model)
        .ok_or_else(|| "embedding model unavailable".into())
        .and_then(|(_, model)| forward(model, &request.text));
    let _ = request.reply.send(result);
}

#[cfg(test)]
mod tests;
