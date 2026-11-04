use axum::body::Bytes;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::Notify;

pub(crate) mod model;
pub(crate) use model::{Frame, Kind, Push, Traffic};

struct Shared {
    model: Mutex<model::Model>,
    ready: Notify,
    space: Notify,
}

#[derive(Clone)]
pub(crate) struct Scheduler(Arc<Shared>);

impl Scheduler {
    pub fn new() -> Self {
        Self(Arc::new(Shared { model: Mutex::new(model::Model::default()), ready: Notify::new(), space: Notify::new() }))
    }

    pub fn register(&self, id: String, traffic: Traffic, stream: bool) -> bool {
        self.0.model.lock().register(id, traffic, stream)
    }

    pub async fn send(&self, id: &str, bytes: Bytes, kind: Kind) -> Result<(), Push> {
        let mut frame = Frame { bytes, kind };
        loop {
            let space = self.0.space.notified();
            tokio::pin!(space);
            space.as_mut().enable();
            let result = { self.0.model.lock().push(id, frame) };
            match result {
                Ok(()) => { self.0.ready.notify_one(); return Ok(()); }
                Err((Push::Wait, returned)) => { frame = returned; space.await; }
                Err((reason, _)) => return Err(reason),
            }
        }
    }

    pub async fn recv(&self) -> Option<Bytes> {
        loop {
            let ready = self.0.ready.notified();
            let (frame, closed) = { let mut model = self.0.model.lock(); (model.pop(), model.closed()) };
            if frame.is_some() { self.0.space.notify_waiters(); return frame; }
            if closed { return None; }
            ready.await;
        }
    }

    pub fn overflow(&self, id: &str, end: Bytes) {
        self.0.model.lock().finish_overflow(id, end);
        self.0.ready.notify_one();
        self.0.space.notify_waiters();
    }

    pub fn remove(&self, id: &str) {
        self.0.model.lock().remove(id);
        self.0.space.notify_waiters();
    }

    pub fn close(&self) {
        self.0.model.lock().close();
        self.0.ready.notify_one();
        self.0.space.notify_waiters();
    }
}
