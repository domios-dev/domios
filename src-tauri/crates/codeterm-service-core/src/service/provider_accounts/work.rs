use crate::service::ServiceContext;

pub(super) struct Receipt {
    #[cfg(any(test, feature = "test-support"))]
    service: ServiceContext,
}
impl Receipt {
    pub fn capture(service: &ServiceContext) -> Self {
        #[cfg(any(test, feature = "test-support"))]
        service
            .ext_or_init::<super::State>(Default::default)
            .work
            .send_modify(|pending| *pending += 1);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = service;
        Self {
            #[cfg(any(test, feature = "test-support"))]
            service: service.clone(),
        }
    }
}
impl Drop for Receipt {
    fn drop(&mut self) {
        #[cfg(any(test, feature = "test-support"))]
        self.service
            .ext_or_init::<super::State>(Default::default)
            .work
            .send_modify(|pending| *pending -= 1);
    }
}
