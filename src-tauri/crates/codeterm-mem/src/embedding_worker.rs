use crate::embedder::local::LocalModel;
use crate::embedding_model::EmbeddingModel;
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

const QUEUE_CAPACITY: usize = 64;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
type Reply = mpsc::SyncSender<Result<Vec<f32>, String>>;

struct Request {
    model: &'static LocalModel,
    text: String,
    reply: Reply,
}

struct Worker {
    bulk: mpsc::SyncSender<Request>,
    query: mpsc::SyncSender<Request>,
    wake: mpsc::SyncSender<()>,
}

struct Receivers {
    bulk: mpsc::Receiver<Request>,
    query: mpsc::Receiver<Request>,
    wake: mpsc::Receiver<()>,
    #[cfg(test)]
    waiting: Option<mpsc::Sender<()>>,
}

fn channels() -> (Worker, Receivers) {
    let (bulk, bulk_rx) = mpsc::sync_channel(QUEUE_CAPACITY);
    let (query, query_rx) = mpsc::sync_channel(QUEUE_CAPACITY);
    let (wake, wake_rx) = mpsc::sync_channel(1);
    (
        Worker { bulk, query, wake },
        Receivers {
            bulk: bulk_rx,
            query: query_rx,
            wake: wake_rx,
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
        self.query.try_recv().or_else(|_| self.bulk.try_recv()).ok()
    }

    fn next(&self) -> Result<Request, mpsc::RecvError> {
        loop {
            let _ = self.wake.try_recv();
            if let Some(request) = self.try_next() {
                return Ok(request);
            }
            #[cfg(test)]
            if let Some(waiting) = &self.waiting {
                let _ = waiting.send(());
            }
            self.wake.recv()?;
        }
    }
}

fn worker() -> Result<&'static Worker, String> {
    static WORKER: OnceLock<Result<Worker, String>> = OnceLock::new();
    WORKER
        .get_or_init(Worker::start)
        .as_ref()
        .map_err(Clone::clone)
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
    let mut model: Option<(&'static LocalModel, M)> = None;
    while let Ok(request) = receivers.next() {
        let mut pending = Some(request);
        // Scoped workers are joined before the coordinator can block on its channel.
        let result = pool().build_scoped(
            |thread| thread.run(),
            |pool| {
                while let Some(request) = pending.take() {
                    process(&mut model, request, &load, |model, text| {
                        pool.install(|| forward(model, text))
                    });
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
    model: &mut Option<(&'static LocalModel, M)>,
    request: Request,
    load: &impl Fn(&'static LocalModel) -> Result<M, String>,
    forward: impl FnOnce(&M, &str) -> Result<Vec<f32>, String>,
) {
    if model
        .as_ref()
        .is_none_or(|(loaded, _)| *loaded != request.model)
    {
        *model = None;
        match load(request.model) {
            Ok(loaded) => {
                *model = Some((request.model, loaded));
                log::debug!("[embed] loaded candle model {}", request.model.model);
            }
            Err(error) => {
                log::warn!("[embed] model unavailable: {error}");
                let _ = request.reply.send(Err(error));
                return;
            }
        }
    }
    let result = model
        .as_ref()
        .ok_or_else(|| "embedding model unavailable".into())
        .and_then(|(_, model)| forward(model, &request.text));
    let _ = request.reply.send(result);
}

#[cfg(test)]
mod tests;
