use std::future::Future;
use std::pin::Pin;

pub mod admission;
mod authentication;
mod controller;
mod dial;
pub use controller::{connect_enrollment, EnrollmentClient};
pub(crate) use dial::direct_address_allowed;
pub use dial::CloudRoute;
mod listener;
pub use authentication::{
    BootstrapResponder, ChannelProof, CloudIngressAuthority, IngressError, IngressReady,
};
pub use listener::{start, CloudIngressHandle};

pub const MAX_ENROLLMENT_FRAME_BYTES: usize = 64 * 1024;

pub struct EnrollmentSession {
    remote_installation_key: [u8; 32],
    local_installation_key: [u8; 32],
    channel_binding: [u8; 32],
}

impl EnrollmentSession {
    pub fn remote_installation_key(&self) -> &[u8; 32] {
        &self.remote_installation_key
    }

    pub fn local_installation_key(&self) -> &[u8; 32] {
        &self.local_installation_key
    }

    pub fn channel_binding(&self) -> &[u8; 32] {
        &self.channel_binding
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnrollmentOperation {
    Discover,
    Apply,
    Status,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnrollmentError {
    Rejected,
    AlreadyApplied,
    Unavailable,
    InvalidFrame,
}

pub type EnrollmentResult<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, EnrollmentError>> + Send + 'a>>;

pub trait EnrollmentHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        session: &'a EnrollmentSession,
        op: EnrollmentOperation,
        payload: &'a [u8],
    ) -> EnrollmentResult<'a>;
}

#[cfg(test)]
mod tests;
