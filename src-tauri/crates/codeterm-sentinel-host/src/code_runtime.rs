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
    if code.src.as_ref().is_some_and(|source| source.len() > 16384) {
        return Err(RuntimeFailure::new("budget_exhausted"));
    }
    if cancellation.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(RuntimeFailure::new("skip:lease_cancelled"));
    }
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

pub fn run_from_directory(
    code: &Code,
    directory: &std::path::Path,
    input: &[u8],
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
) -> Result<Output, RuntimeFailure> {
    platform_supported(std::env::consts::OS)?;
    #[cfg(target_os = "macos")]
    let code = {
        let mut code = code.clone();
        if let Some(file) = code.file.take() {
            if code.src.is_some() {
                return Err(RuntimeFailure::new("contract_violation"));
            }
            code.src = Some(source::read(directory, &file)?);
        }
        code
    };
    #[cfg(not(target_os = "macos"))]
    let code = {
        let _ = directory;
        code.clone()
    };
    run(&code, input, timeout, owner, cancellation)
}

#[cfg(target_os = "macos")]
mod source;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;
