use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

use super::operation::Context;
use super::VmError;

pub(in crate::js_runtime) struct CallMutex<T> {
    inner: Mutex<T>,
    waiters: Mutex<Vec<Arc<Context>>>,
    #[cfg(test)]
    pub(in crate::js_runtime) waiting: Mutex<Option<std::sync::mpsc::Sender<()>>>,
}

impl<T> CallMutex<T> {
    pub(in crate::js_runtime) fn new(value: T) -> Self {
        Self {
            inner: Mutex::new(value),
            waiters: Mutex::new(Vec::new()),
            #[cfg(test)]
            waiting: Mutex::new(None),
        }
    }

    pub(in crate::js_runtime) fn lock(&self) -> Guard<'_, T> {
        Guard {
            inner: Some(self.inner.lock()),
            owner: self,
        }
    }

    pub(in crate::js_runtime) fn try_lock(&self) -> Option<Guard<'_, T>> {
        self.inner.try_lock().map(|inner| Guard {
            inner: Some(inner),
            owner: self,
        })
    }

    pub(in crate::js_runtime) fn try_lock_for(
        &self,
        timeout: std::time::Duration,
    ) -> Option<Guard<'_, T>> {
        self.inner.try_lock_for(timeout).map(|inner| Guard {
            inner: Some(inner),
            owner: self,
        })
    }

    pub(in crate::js_runtime) fn enter(
        &self,
        context: &Arc<Context>,
    ) -> Result<Guard<'_, T>, VmError> {
        self.waiters.lock().push(context.clone());
        let _waiter = Waiter {
            owner: self,
            context,
        };
        loop {
            let generation = context.cancellation().generation();
            context.check()?;
            if let Some(guard) = self.try_lock() {
                context.check()?;
                return Ok(guard);
            }
            #[cfg(test)]
            if let Some(waiting) = self.waiting.lock().take() {
                let _ = waiting.send(());
            }
            context.cancellation().wait_for_change(
                generation,
                context
                    .deadline
                    .saturating_duration_since(std::time::Instant::now()),
            );
        }
    }
}

struct Waiter<'a, T> {
    owner: &'a CallMutex<T>,
    context: &'a Arc<Context>,
}

impl<T> Drop for Waiter<'_, T> {
    fn drop(&mut self) {
        self.owner
            .waiters
            .lock()
            .retain(|entry| !Arc::ptr_eq(entry, self.context));
    }
}

pub(in crate::js_runtime) struct Guard<'a, T> {
    // The guard exists until Drop unlocks it before waking waiters.
    inner: Option<MutexGuard<'a, T>>,
    owner: &'a CallMutex<T>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner.as_deref().unwrap()
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.inner.as_deref_mut().unwrap()
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        drop(self.inner.take());
        let waiters = self.owner.waiters.lock().clone();
        for context in waiters {
            context.cancellation().notify_change();
        }
    }
}
