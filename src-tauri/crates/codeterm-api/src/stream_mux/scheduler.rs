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
        Self(Arc::new(Shared {
            model: Mutex::new(model::Model::default()),
            ready: Notify::new(),
            space: Notify::new(),
        }))
    }

    async fn edit<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut model::Model) -> T + Send + 'static,
    ) -> Option<T> {
        {
            if let Some(mut model) = self.0.model.try_lock() {
                return Some(work(&mut model));
            }
        }
        let shared = self.0.clone();
        tokio::task::spawn_blocking(move || {
            work(&mut super::locks::lock(&shared.model, "scheduler"))
        })
        .await
        .ok()
    }

    fn cleanup(&self, work: impl FnOnce(&mut model::Model) + Send + 'static) {
        {
            if let Some(mut model) = self.0.model.try_lock() {
                work(&mut model);
                self.0.ready.notify_one();
                self.0.space.notify_waiters();
                return;
            }
        }
        let shared = self.0.clone();
        super::locks::cleanup(move || {
            work(&mut super::locks::lock(&shared.model, "scheduler"));
            shared.ready.notify_one();
            shared.space.notify_waiters();
        });
    }

    pub fn register(&self, id: String, traffic: Traffic, stream: bool) -> bool {
        super::locks::lock(&self.0.model, "scheduler").register(id, traffic, stream)
    }

    pub async fn send(&self, id: &str, bytes: Bytes, kind: Kind) -> Result<(), Push> {
        let mut frame = Frame { bytes, kind };
        loop {
            let space = self.0.space.notified();
            tokio::pin!(space);
            space.as_mut().enable();
            let channel = id.to_owned();
            let result = self
                .edit(move |model| model.push(&channel, frame))
                .await
                .ok_or(Push::Closed)?;
            match result {
                Ok(()) => {
                    self.0.ready.notify_one();
                    return Ok(());
                }
                Err((Push::Wait, returned)) => {
                    frame = returned;
                    space.await;
                }
                Err((reason, _)) => return Err(reason),
            }
        }
    }

    pub async fn recv(&self) -> Option<Bytes> {
        loop {
            let ready = self.0.ready.notified();
            let (frame, closed) = self.edit(|model| (model.pop(), model.closed())).await?;
            if frame.is_some() {
                self.0.space.notify_waiters();
                return frame;
            }
            if closed {
                return None;
            }
            ready.await;
        }
    }

    pub async fn overflow(&self, id: &str, end: Bytes) {
        let channel = id.to_owned();
        let _ = self
            .edit(move |model| model.finish_overflow(&channel, end))
            .await;
        self.0.ready.notify_one();
        self.0.space.notify_waiters();
    }

    pub fn remove(&self, id: &str) {
        let channel = id.to_owned();
        self.cleanup(move |model| model.remove(&channel));
    }

    pub fn close(&self) {
        self.cleanup(|model| model.close());
    }
}

#[cfg(test)]
impl Scheduler {
    pub(super) fn with_model_held_for_test(&self, hold: impl FnOnce()) {
        let _guard = self.0.model.lock();
        hold();
    }
}
