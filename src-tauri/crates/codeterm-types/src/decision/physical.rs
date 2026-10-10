use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use super::DecisionError;

pub trait RequestObserver: Send + Sync {
    fn before_request(&self) -> Result<(), DecisionError>;
    fn deadline(&self) -> Option<Instant> {
        None
    }
}

thread_local! {
    static OBSERVER: RefCell<Option<Arc<dyn RequestObserver>>> = const { RefCell::new(None) };
}

pub struct RequestScope {
    previous: Option<Arc<dyn RequestObserver>>,
    thread: PhantomData<Rc<()>>,
}

impl RequestScope {
    pub fn enter(observer: Arc<dyn RequestObserver>) -> Self {
        Self {
            previous: OBSERVER.with(|slot| slot.replace(Some(observer))),
            thread: PhantomData,
        }
    }
}

impl Drop for RequestScope {
    fn drop(&mut self) {
        OBSERVER.with(|slot| slot.replace(self.previous.take()));
    }
}

pub fn current() -> Option<Arc<dyn RequestObserver>> {
    OBSERVER.with(|slot| slot.borrow().clone())
}

pub fn deadline() -> Option<Instant> {
    current().and_then(|observer| observer.deadline())
}

pub fn before_request() -> Result<(), DecisionError> {
    current().map_or(Ok(()), |observer| observer.before_request())
}
