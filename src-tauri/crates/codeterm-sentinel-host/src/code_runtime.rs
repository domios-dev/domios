use crate::probe::{cancellation::Cancellation, owner::ProbeResourceOwner, Output};
use codeterm_blueprint_types::blueprint_document::{Code, Language};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug)]
pub struct RuntimeFailure {
    pub reason: &'static str,
    pub uncertain: bool,
    #[cfg(any(test, feature = "test-support"))]
    pub stderr_tail: String,
}

impl RuntimeFailure {
    fn observed(reason: &'static str, _stderr: &str) -> Self {
        let failure = Self::new(reason);
        #[cfg(any(test, feature = "test-support"))]
        let failure = Self {
            stderr_tail: crate::probe::test_receipt::stderr_tail(_stderr),
            ..failure
        };
        failure
    }

    fn new(reason: &'static str) -> Self {
        Self {
            reason,
            uncertain: false,
            #[cfg(any(test, feature = "test-support"))]
            stderr_tail: String::new(),
        }
    }
}

#[cfg(test)]
pub fn run(
    code: &Code,
    input: &[u8],
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
) -> Result<Output, RuntimeFailure> {
    let receipt = crate::probe::test_receipt::Receipt::start(
        &format!("native-code:{:?}", code.lang),
        cancellation,
    );
    let result = run_with_output(
        code,
        input,
        Duration::from_secs(60),
        timeout,
        owner,
        cancellation,
        None,
    );
    let outcome = match &result {
        Ok(output) if output.success => "done".into(),
        Ok(output) => format!("nonzero:{:?}", output.exit_code),
        Err(failure) => failure.reason.to_owned(),
    };
    let detail = match &result {
        Ok(output) => format!(
            "stderr_tail={}",
            crate::probe::test_receipt::stderr_tail(&output.stderr_text())
        ),
        Err(failure) => {
            format!(
                "runtime_failure={failure:?}; stderr_tail={:?}",
                failure.stderr_tail
            )
        }
    };
    receipt.finish(
        result.as_ref().is_ok_and(|output| output.success),
        &outcome,
        &detail,
    );
    result
}

pub fn run_with_output(
    code: &Code,
    input: &[u8],
    limit: Duration,
    timeout: Duration,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    output: Option<&codeterm_blueprint_types::blueprint_document::Port>,
) -> Result<Output, RuntimeFailure> {
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
        return Err(RuntimeFailure::new("contract_violation"));
    }
    if !codeterm_blueprint_types::blueprint_document::code_contract::has_main(code) {
        return Err(RuntimeFailure::new("contract_violation"));
    }
    child::run(code, input, limit, timeout, owner, cancellation, output)
}

mod child;
#[cfg(test)]
mod tests;
