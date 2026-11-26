use std::future::Future;
use std::pin::Pin;

mod authentication;
mod controller;
mod dial;
pub use controller::{connect_enrollment, EnrollmentClient};
pub use dial::CloudRoute;
mod listener;
pub use authentication::{
    BootstrapResponder, ChannelProof, CloudIngressAuthority, IngressError, IngressReady,
};
pub use listener::{start, CloudIngressHandle};

pub const MAX_ENROLLMENT_FRAME_BYTES: usize = 64 * 1024;

pub(crate) fn valid_endpoint(origin: &str, header_name: &str) -> bool {
    if origin.len() > 300 || header_name.len() > 128 || header_name.is_empty() {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(origin) else {
        return false;
    };
    let Some(host) = url.domain() else {
        return false;
    };
    let labels = host.split('.');
    url.scheme() == "https"
        && origin == url.origin().ascii_serialization()
        && url.port() != Some(0)
        && host.len() <= 253
        && host.contains('.')
        && labels.clone().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && !["localhost", "local", "internal", "ts.net", "home.arpa"]
            .iter()
            .any(|suffix| {
                host == *suffix
                    || host
                        .strip_suffix(*suffix)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
        && reqwest::header::HeaderName::from_lowercase(header_name.as_bytes()).is_ok()
        && (header_name == "cookie"
            || (header_name.starts_with("x-")
                && !["x-forwarded-", "x-domios-", "x-codeterm-"]
                    .iter()
                    .any(|prefix| header_name.starts_with(*prefix))))
}

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
