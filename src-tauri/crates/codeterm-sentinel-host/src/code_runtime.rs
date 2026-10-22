use crate::probe::{cancellation::Cancellation, owner::ProbeResourceOwner, Output};
use codeterm_blueprint_types::blueprint_document::{Code, Language};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub struct RuntimeFailure {
    pub reason: &'static str,
    pub uncertain: bool,
}

impl RuntimeFailure {
    fn new(reason: &'static str) -> Self {
        Self {
            reason,
            uncertain: false,
        }
    }
}

pub fn platform_supported(platform: &str) -> Result<(), RuntimeFailure> {
    if platform == "macos" {
        Ok(())
    } else {
        Err(RuntimeFailure::new("unsupported_platform"))
    }
}

pub fn run(
    code: &Code,
    input: &[u8],
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
) -> Result<Output, RuntimeFailure> {
    platform_supported(std::env::consts::OS)?;
    if !code.caps.is_empty()
        || code.file.is_some()
        || code.src.is_none()
        || matches!(code.lang, Language::Shell)
    {
        return Err(RuntimeFailure::new("sandbox_denied"));
    }
    #[cfg(target_os = "macos")]
    {
        macos::run(code, input, timeout, owner, cancellation)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (input, timeout, owner, cancellation);
        Err(RuntimeFailure::new("unsupported_platform"))
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;
