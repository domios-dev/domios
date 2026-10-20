use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::mpsc::{self, RecvTimeoutError, SendError};
use std::time::Instant;

#[cfg(feature = "decision-fixture-clock")]
mod fixture {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex, Weak};
    use std::time::Duration;

    use super::*;

    const FIXTURE_EVENT_LIVENESS_BOUND: Duration = Duration::from_secs(10);

    thread_local! {
        static CLOCK: RefCell<Option<FixtureClock>> = const { RefCell::new(None) };
    }

    pub(super) fn current() -> Option<FixtureClock> {
        CLOCK.with(|clock| clock.borrow().clone())
    }

    pub(super) fn replace(clock: Option<FixtureClock>) -> Option<FixtureClock> {
        CLOCK.with(|slot| slot.replace(clock))
    }

    trait Wake: Send + Sync {
        fn wake(&self);
    }

    struct Tick {
        now: Instant,
        listeners: Vec<Weak<dyn Wake>>,
    }

    #[derive(Clone)]
    pub struct FixtureClock(Arc<Mutex<Tick>>);

    impl FixtureClock {
        pub fn new(origin: Instant) -> Self {
            Self(Arc::new(Mutex::new(Tick {
                now: origin,
                listeners: Vec::new(),
            })))
        }

        pub fn now(&self) -> Instant {
            self.0.lock().unwrap().now
        }

        pub fn enter(&self) -> Scope {
            Context {
                clock: Some(self.clone()),
            }
            .enter()
        }

        pub fn advance_to(&self, now: Instant) {
            let listeners = {
                let mut tick = self.0.lock().unwrap();
                if now < tick.now {
                    drop(tick);
                    panic!("fixture clock cannot move backwards");
                }
                tick.now = now;
                tick.listeners
                    .retain(|listener| listener.strong_count() > 0);
                tick.listeners
                    .iter()
                    .filter_map(Weak::upgrade)
                    .collect::<Vec<_>>()
            };
            for listener in listeners {
                listener.wake();
            }
        }

        pub(super) fn channel<T: Send + 'static>(&self) -> (Sender<T>, Receiver<T>) {
            let queue = Arc::new(Queue {
                clock: self.clone(),
                state: Mutex::new(State {
                    values: VecDeque::new(),
                    senders: 1,
                    closed_at: None,
                    receiver_alive: true,
                }),
                changed: Condvar::new(),
            });
            let wake: Arc<dyn Wake> = queue.clone();
            self.0.lock().unwrap().listeners.push(Arc::downgrade(&wake));
            (Sender(queue.clone()), Receiver(queue))
        }
    }

    struct State<T> {
        values: VecDeque<(Instant, T)>,
        senders: usize,
        closed_at: Option<Instant>,
        receiver_alive: bool,
    }

    struct Queue<T> {
        clock: FixtureClock,
        state: Mutex<State<T>>,
        changed: Condvar,
    }

    impl<T: Send> Wake for Queue<T> {
        fn wake(&self) {
            let _state = self.state.lock().unwrap();
            self.changed.notify_all();
        }
    }

    pub struct Sender<T: Send>(Arc<Queue<T>>);
    pub struct Receiver<T: Send>(Arc<Queue<T>>);

    impl<T: Send> Clone for Sender<T> {
        fn clone(&self) -> Self {
            self.0.state.lock().unwrap().senders += 1;
            Self(self.0.clone())
        }
    }

    impl<T: Send> Sender<T> {
        pub(super) fn send(&self, value: T) -> Result<(), SendError<T>> {
            let mut state = self.0.state.lock().unwrap();
            if !state.receiver_alive {
                return Err(SendError(value));
            }
            state.values.push_back((self.0.clock.now(), value));
            self.0.changed.notify_all();
            Ok(())
        }
    }

    impl<T: Send> Drop for Sender<T> {
        fn drop(&mut self) {
            let mut state = self.0.state.lock().unwrap();
            state.senders -= 1;
            if state.senders == 0 {
                state.closed_at = Some(self.0.clock.now());
                self.0.changed.notify_all();
            }
        }
    }

    impl<T: Send> Receiver<T> {
        pub(super) fn recv_until(&self, deadline: Instant) -> Result<T, RecvTimeoutError> {
            let failure_deadline = Instant::now() + FIXTURE_EVENT_LIVENESS_BOUND;
            let mut state = self.0.state.lock().unwrap();
            loop {
                if state.values.front().is_some_and(|(at, _)| *at < deadline) {
                    return Ok(state.values.pop_front().unwrap().1);
                }
                if state.closed_at.is_some_and(|at| at < deadline) && state.values.is_empty() {
                    return Err(RecvTimeoutError::Disconnected);
                }
                if self.0.clock.now() >= deadline {
                    return Err(RecvTimeoutError::Timeout);
                }
                let remaining = failure_deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    drop(state);
                    panic!("fixture_event_liveness_exceeded");
                }
                state = self.0.changed.wait_timeout(state, remaining).unwrap().0;
            }
        }
    }

    impl<T: Send> Drop for Receiver<T> {
        fn drop(&mut self) {
            let mut state = self.0.state.lock().unwrap();
            state.receiver_alive = false;
            state.values.clear();
        }
    }
}

#[cfg(feature = "decision-fixture-clock")]
pub use fixture::FixtureClock;

#[derive(Clone, Default)]
pub struct Context {
    #[cfg(feature = "decision-fixture-clock")]
    clock: Option<FixtureClock>,
}

pub struct Scope {
    #[cfg(feature = "decision-fixture-clock")]
    previous: Option<FixtureClock>,
    thread: PhantomData<Rc<()>>,
}

pub fn now() -> Instant {
    #[cfg(feature = "decision-fixture-clock")]
    if let Some(clock) = fixture::current() {
        return clock.now();
    }
    Instant::now()
}

pub fn capture() -> Context {
    Context {
        #[cfg(feature = "decision-fixture-clock")]
        clock: fixture::current(),
    }
}

impl Context {
    pub fn enter(&self) -> Scope {
        Scope {
            #[cfg(feature = "decision-fixture-clock")]
            previous: fixture::replace(self.clock.clone()),
            thread: PhantomData,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        #[cfg(feature = "decision-fixture-clock")]
        fixture::replace(self.previous.take());
    }
}

pub enum Sender<T: Send> {
    Real(mpsc::Sender<T>),
    #[cfg(feature = "decision-fixture-clock")]
    Fixture(fixture::Sender<T>),
}

pub enum Receiver<T: Send> {
    Real(mpsc::Receiver<T>),
    #[cfg(feature = "decision-fixture-clock")]
    Fixture(fixture::Receiver<T>),
}

pub fn channel<T: Send + 'static>() -> (Sender<T>, Receiver<T>) {
    #[cfg(feature = "decision-fixture-clock")]
    if let Some(clock) = fixture::current() {
        let (sender, receiver) = clock.channel();
        return (Sender::Fixture(sender), Receiver::Fixture(receiver));
    }
    let (sender, receiver) = mpsc::channel();
    (Sender::Real(sender), Receiver::Real(receiver))
}

impl<T: Send> Clone for Sender<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Real(sender) => Self::Real(sender.clone()),
            #[cfg(feature = "decision-fixture-clock")]
            Self::Fixture(sender) => Self::Fixture(sender.clone()),
        }
    }
}

impl<T: Send> Sender<T> {
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        match self {
            Self::Real(sender) => sender.send(value),
            #[cfg(feature = "decision-fixture-clock")]
            Self::Fixture(sender) => sender.send(value),
        }
    }
}

impl<T: Send> Receiver<T> {
    pub fn recv_until(&self, deadline: Instant) -> Result<T, RecvTimeoutError> {
        match self {
            Self::Real(receiver) => {
                receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            }
            #[cfg(feature = "decision-fixture-clock")]
            Self::Fixture(receiver) => receiver.recv_until(deadline),
        }
    }
}
