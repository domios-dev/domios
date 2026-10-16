use crate::embedding_model::EmbeddingModel;
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

const QUEUE_CAPACITY: usize = 64;
const IDLE_TIMEOUT: Duration = Duration::from_secs(1800);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
type Reply = mpsc::SyncSender<Result<Vec<f32>, String>>;

struct Request {
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
        },
    )
}

impl Worker {
    fn start() -> Result<Self, String> {
        let (worker, receivers) = channels();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .thread_name(|index| format!("mem-embed-cpu-{index}"))
            .build()
            .map_err(|e| format!("start embedding CPU pool: {e}"))?;
        std::thread::Builder::new()
            .name("mem-embed".into())
            .spawn(move || {
                run(
                    receivers,
                    IDLE_TIMEOUT,
                    EmbeddingModel::load,
                    |model, text| pool.install(|| model.embed(text)),
                )
            })
            .map_err(|e| format!("start embedding worker: {e}"))?;
        Ok(worker)
    }

    fn embed(&self, text: &str, query: bool) -> Result<Vec<f32>, String> {
        let (reply, response) = mpsc::sync_channel(1);
        let request = Request {
            text: text.into(),
            reply,
        };
        if query {
            self.query
                .send(request)
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

    fn query_embed(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed(text, true)
    }
}

impl Receivers {
    fn next(&self, deadline: Option<Instant>) -> Result<Request, mpsc::RecvTimeoutError> {
        loop {
            if let Ok(request) = self.query.try_recv().or_else(|_| self.bulk.try_recv()) {
                return Ok(request);
            }
            match deadline {
                Some(deadline) => self
                    .wake
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))?,
                None => self
                    .wake
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?,
            }
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

pub(crate) fn embed(text: &str) -> Result<Vec<f32>, String> {
    worker()?.embed(text, false)
}

pub(crate) fn query_embed(text: &str) -> Result<Vec<f32>, String> {
    worker()?.query_embed(text)
}

fn run<M>(
    receivers: Receivers,
    idle: Duration,
    load: impl Fn() -> Result<M, String>,
    forward: impl Fn(&M, &str) -> Result<Vec<f32>, String>,
) {
    let mut model = None;
    let mut deadline = None;
    loop {
        let request = match receivers.next(deadline) {
            Ok(request) => request,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                model = None;
                deadline = None;
                log::debug!("[embed] unloaded idle candle model");
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if model.is_none() {
            match load() {
                Ok(loaded) => {
                    model = Some(loaded);
                    log::debug!("[embed] loaded candle model");
                }
                Err(error) => {
                    log::warn!("[embed] model unavailable: {error}");
                    let _ = request.reply.send(Err(error));
                    continue;
                }
            }
        }
        let result = model
            .as_ref()
            .ok_or_else(|| "embedding model unavailable".into())
            .and_then(|model| forward(model, &request.text));
        let _ = request.reply.send(result);
        deadline = Some(Instant::now() + idle);
    }
}

#[cfg(test)]
mod tests;
