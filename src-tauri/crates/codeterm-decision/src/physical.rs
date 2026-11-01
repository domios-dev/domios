use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use super::DecisionError;

pub trait RequestObserver: Send + Sync {
    fn before_request(&self) -> Result<(), DecisionError>;
    fn effects_allowed(&self) -> bool {
        true
    }
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

struct ObservationOnly(Instant);
impl RequestObserver for ObservationOnly {
    fn before_request(&self) -> Result<(), DecisionError> {
        Err(DecisionError::Unsupported)
    }
    fn effects_allowed(&self) -> bool {
        false
    }
    fn deadline(&self) -> Option<Instant> {
        Some(self.0)
    }
}

impl RequestScope {
    pub fn enter(observer: Arc<dyn RequestObserver>) -> Self {
        let observer = match current().filter(|outer| !outer.effects_allowed()) {
            Some(outer) => {
                let bound = [outer.deadline(), observer.deadline()]
                    .into_iter()
                    .flatten()
                    .min();
                bound
                    .map(|deadline| Arc::new(ObservationOnly(deadline)) as Arc<dyn RequestObserver>)
                    .unwrap_or(outer)
            }
            None => observer,
        };
        Self {
            previous: OBSERVER.with(|slot| slot.replace(Some(observer))),
            thread: PhantomData,
        }
    }

    pub fn observation_only(deadline: Instant) -> Self {
        let deadline = current()
            .and_then(|observer| observer.deadline())
            .map_or(deadline, |outer| outer.min(deadline));
        Self::enter(Arc::new(ObservationOnly(deadline)))
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

pub fn effects_allowed() -> bool {
    current().is_none_or(|observer| observer.effects_allowed())
}

#[cfg(test)]
mod tests;
