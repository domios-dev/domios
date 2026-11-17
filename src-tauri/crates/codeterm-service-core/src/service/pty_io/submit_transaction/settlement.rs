use crate::service::inject_verify::{InjectionError, InjectionResult};
use std::time::Duration;

pub(super) fn settle_exact_turn(
    result: Result<InjectionResult, InjectionError>,
    exact_turn_present: bool,
    queued_payload_bytes: usize,
    queued_submit_keys: usize,
    elapsed: Duration,
) -> Result<InjectionResult, InjectionError> {
    match result {
        Ok(mut result) if exact_turn_present => {
            result.submit_observed = true;
            result.submit_anomaly = false;
            Ok(result)
        }
        Err(InjectionError::SubmitNeverConfirmed { .. })
            if exact_turn_present && queued_payload_bytes > 0 =>
        {
            // Exact append evidence settles an enqueued payload despite a late write error.
            Ok(InjectionResult {
                bytes_written: queued_payload_bytes,
                echo_observed: false,
                submit_observed: true,
                submit_anomaly: false,
                submit_key_writes: queued_submit_keys,
                elapsed,
            })
        }
        result => result,
    }
}
