use super::StepRequest;
use std::cell::RefCell;

#[derive(Clone)]
pub struct StepInputContext {
    pub plugin: String,
    pub request: StepRequest,
    pub submitting: bool,
    pub deadline: std::time::Instant,
    pub cancellation: crate::converter::ConversionCancellation,
}
thread_local! { static CURRENT: RefCell<Option<StepInputContext>> = const { RefCell::new(None) }; }
pub(crate) struct StepInputScope(Option<StepInputContext>);
impl StepInputScope {
    pub(crate) fn enter(plugin: &str, call: &super::StepCall<'_>, submitting: bool) -> Self {
        Self(CURRENT.with(|slot| {
            slot.replace(Some(StepInputContext {
                plugin: plugin.into(),
                request: call.request.clone(),
                submitting,
                deadline: call.deadline,
                cancellation: call.cancellation.clone(),
            }))
        }))
    }
}
impl Drop for StepInputScope {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
pub fn current_step_input(plugin: &str) -> Option<StepInputContext> {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|context| context.plugin == plugin)
            .cloned()
    })
}

pub trait RunInputHost: Send + Sync {
    fn bind(
        &self,
        plugin: &str,
        context: &StepInputContext,
        handle: &str,
        destination: &str,
        control: &crate::binary_transfer::InputPreparation,
    ) -> crate::binary_transfer::Result<crate::binary_transfer::TransferInputGrant>;
}
pub struct RunInputs(pub std::sync::Arc<dyn RunInputHost>);
