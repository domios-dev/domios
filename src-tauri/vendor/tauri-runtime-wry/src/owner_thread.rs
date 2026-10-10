use std::{
    fmt,
    mem::ManuallyDrop,
    ops::Deref,
    thread::{self, ThreadId},
};

pub(crate) struct OwnerThread<T> {
    value: ManuallyDrop<T>,
    thread: ThreadId,
}

impl<T> OwnerThread<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value: ManuallyDrop::new(value),
            thread: thread::current().id(),
        }
    }
}

impl<T> Drop for OwnerThread<T> {
    fn drop(&mut self) {
        if thread::current().id() == self.thread {
            // The final owner-thread drop preserves the native payload's destruction contract.
            unsafe { ManuallyDrop::drop(&mut self.value) };
        }
        // Late off-owner drops deliberately leak native data because its event loop may have exited.
    }
}

impl<T> Deref for OwnerThread<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> fmt::Debug for OwnerThread<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OwnerThread")
            .field("thread", &self.thread)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "owner_thread/tests.rs"]
mod tests;
