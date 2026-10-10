use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use parking_lot::Mutex;
use tokio::sync::mpsc::Receiver;

#[derive(Default)]
pub struct BackendOutput {
    state: Mutex<OutputState>,
}

#[derive(Default)]
struct OutputState {
    receiver: Option<Receiver<Vec<u8>>>,
    generation: u64,
    waiter: Option<Waker>,
    closed: bool,
}

impl BackendOutput {
    pub fn install(&self, receiver: Receiver<Vec<u8>>) {
        let waiter = {
            let mut state = self.state.lock();
            state.receiver = Some(receiver);
            state.waiter.take()
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub fn is_configured(&self) -> bool {
        let state = self.state.lock();
        state.receiver.is_some() || state.closed
    }

    pub fn close(&self) {
        let waiter = {
            let mut state = self.state.lock();
            state.closed = true;
            state.receiver = None;
            state.waiter.take()
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub fn read(&self) -> BackendRead<'_> {
        let (generation, waiter) = {
            let mut state = self.state.lock();
            state.generation += 1;
            (state.generation, state.waiter.take())
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
        BackendRead {
            output: self,
            generation,
        }
    }

    fn poll_read(&self, generation: u64, cx: &mut Context<'_>) -> Poll<Result<Vec<u8>, String>> {
        let mut state = self.state.lock();
        if state.generation != generation {
            return Poll::Ready(Err("PTY read superseded".to_string()));
        }
        if state.closed {
            return Poll::Ready(Err("PTY closed".to_string()));
        }
        state.waiter = Some(cx.waker().clone());
        let Some(receiver) = state.receiver.as_mut() else {
            return Poll::Pending;
        };
        match receiver.poll_recv(cx) {
            Poll::Ready(Some(mut data)) => {
                while data.len() < 64 * 1024 {
                    match receiver.try_recv() {
                        Ok(mut next) => data.append(&mut next),
                        Err(_) => break,
                    }
                }
                state.waiter = None;
                Poll::Ready(Ok(data))
            }
            Poll::Ready(None) => {
                state.waiter = None;
                Poll::Ready(Err("PTY closed".to_string()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct BackendRead<'a> {
    output: &'a BackendOutput,
    generation: u64,
}

impl Future for BackendRead<'_> {
    type Output = Result<Vec<u8>, String>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.output.poll_read(self.generation, cx)
    }
}

impl Drop for BackendRead<'_> {
    fn drop(&mut self) {
        let mut state = self.output.state.lock();
        if state.generation == self.generation {
            state.waiter = None;
        }
    }
}

#[cfg(test)]
mod tests;
