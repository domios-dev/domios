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
    #[cfg(test)]
    contended_finished: Notify,
}

impl Shared {
    fn edit_locked<T>(&self, work: impl FnOnce(&mut model::Model) -> T) -> T {
        let result = {
            let mut model = super::locks::lock(&self.model, "scheduler");
            work(&mut model)
        };
        #[cfg(test)]
        self.contended_finished.notify_one();
        result
    }
}

#[derive(Clone)]
pub(crate) struct Scheduler(Arc<Shared>);

pub(crate) struct Receiver(Arc<Shared>);

impl Scheduler {
    pub fn new() -> (Self, Receiver) {
        let shared = Arc::new(Shared {
            model: Mutex::new(model::Model::default()),
            ready: Notify::new(),
            space: Notify::new(),
            #[cfg(test)]
            contended_finished: Notify::new(),
        });
        (Self(shared.clone()), Receiver(shared))
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
        tokio::task::spawn_blocking(move || shared.edit_locked(work))
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

    pub async fn send(&self, id: &str, bytes: Bytes, kind: Kind, credit: u64) -> Result<(), Push> {
        let mut frame = Frame {
            bytes,
            kind,
            credit,
        };
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

    pub fn credit(&self, received: u64) {
        self.cleanup(move |model| model.credit(received));
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
    pub(super) async fn contended_operation_finished_for_test(&self) {
        self.0.contended_finished.notified().await;
    }

    pub(super) fn with_model_held_for_test(&self, hold: impl FnOnce()) {
        let _guard = self.0.model.lock();
        hold();
    }
}

impl Receiver {
    pub async fn recv(&mut self) -> Option<Bytes> {
        loop {
            let ready = self.0.ready.notified();
            let inspected = {
                if let Some(mut model) = self.0.model.try_lock() {
                    if let Some(frame) = model.pop() {
                        self.0.space.notify_waiters();
                        return Some(frame);
                    }
                    if model.closed() {
                        return None;
                    }
                    true
                } else {
                    false
                }
            };
            if inspected {
                ready.await;
            } else {
                // The sole receiver waits only for producers' finite critical sections.
                let shared = self.0.clone();
                tokio::task::spawn_blocking(move || shared.edit_locked(|_| ()))
                    .await
                    .ok()?;
            }
        }
    }
}
