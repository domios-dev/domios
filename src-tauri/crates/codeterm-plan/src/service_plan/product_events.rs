use super::PlanStore;
use crate::plan::{ExecutionIntent, Plan};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProductEvent {
    PlanCreated,
    TaskDispatched { execution_intent: ExecutionIntent },
}
pub type ProductObserver = Arc<dyn Fn(ProductEvent) + Send + Sync>;

pub fn committed_events(previous: Option<&Plan>, committed: &Plan) -> Vec<ProductEvent> {
    let Some(previous) = previous else {
        return vec![];
    };
    committed
        .tasks
        .iter()
        .flat_map(|task| {
            task.execution_attempts.iter().filter_map(move |attempt| {
                let already_acknowledged = previous
                    .tasks
                    .iter()
                    .find(|old| old.id == task.id)
                    .and_then(|old| {
                        old.execution_attempts
                            .iter()
                            .find(|old| old.id == attempt.id)
                    })
                    .is_some_and(|old| old.acknowledged_at.is_some());
                if attempt.acknowledged_at.is_none() || already_acknowledged {
                    return None;
                }
                let execution_intent = task.interruption.execution_intent.clone();
                Some(ProductEvent::TaskDispatched { execution_intent })
            })
        })
        .collect()
}
impl PlanStore {
    pub fn bind_product_observer(&self, observer: ProductObserver) {
        *self.product_observer.write() = Some(observer);
    }
    pub(in crate::service_plan) fn publish_product_created(&self) {
        let observer = self.product_observer.read().clone();
        if let Some(observer) = observer {
            observer(ProductEvent::PlanCreated);
        }
    }
    pub(super) fn publish_product_commit(&self, previous: Option<&Plan>, committed: &Plan) {
        let observer = self.product_observer.read().clone();
        if let Some(observer) = observer {
            for event in committed_events(previous, committed) {
                observer(event);
            }
        }
    }
}
