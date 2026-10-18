pub use codeterm_types::server_bootstrap::{
    AttachReply, AttachRequest, BootstrapOperation, ProcessProvenance, ServerIdentity, ServerReady,
    BOOTSTRAP_PROTOCOL,
};
use std::io;
pub const FAILURE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

pub fn validate_observation(request: &AttachRequest) -> io::Result<()> {
    if request.operation != BootstrapOperation::Inspect || request.expected.is_none() {
        return Err(io::Error::other(
            "Observation requires Inspect and exact expected incarnation",
        ));
    }
    Ok(())
}

pub fn with_observation_preflight<T>(
    request: &AttachRequest,
    attach: impl FnOnce() -> T,
) -> io::Result<T> {
    validate_observation(request)?;
    Ok(attach())
}

pub fn validate_attach(request: &AttachRequest, ready: &ServerReady) -> io::Result<()> {
    if request.protocol != BOOTSTRAP_PROTOCOL || ready.protocol != BOOTSTRAP_PROTOCOL {
        return Err(io::Error::other("Bootstrap protocol mismatch"));
    }
    if request.api_version != ready.api_version || request.build != ready.build {
        return Err(io::Error::other("Server build/API mismatch"));
    }
    if request.identity != ready.identity
        || request.challenge.is_empty()
        || request.challenge.len() > 128
    {
        return Err(io::Error::other("Server identity/challenge mismatch"));
    }
    if let Some(expected) = &request.expected {
        if expected.process != ready.process || expected.epoch != ready.epoch {
            return Err(io::Error::other("Server incarnation mismatch"));
        }
    } else if request.operation == BootstrapOperation::Shutdown {
        return Err(io::Error::other(
            "Shutdown requires exact server incarnation",
        ));
    }
    Ok(())
}

pub fn validate_reply(
    request: &AttachRequest,
    reply: &AttachReply,
    peer: &ProcessProvenance,
) -> io::Result<()> {
    validate_attach(request, &reply.ready)?;
    if reply.challenge != request.challenge
        || &reply.ready.process != peer
        || reply.ready.epoch.is_empty()
    {
        return Err(io::Error::other("Stale or foreign server publication"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
