use std::sync::Arc;

use codeterm_service_core::service::ServiceContext;
use codeterm_update::application_host::{ApplicationHost, NativeUnavailable};

#[derive(Clone)]
pub struct ApplicationHostState(pub Arc<dyn ApplicationHost>);

pub fn require(svc: &ServiceContext) -> Result<Arc<dyn ApplicationHost>, NativeUnavailable> {
    svc.host_extension::<ApplicationHostState>()
        .map(|state| state.0.clone())
        .filter(|host| host.available())
        .ok_or(NativeUnavailable)
}

#[cfg(test)]
mod tests;
