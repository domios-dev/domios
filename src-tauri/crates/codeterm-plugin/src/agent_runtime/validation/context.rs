use super::*;

pub(super) fn validate(event: &AgentRuntimeEvent) -> Result<(), RuntimeFailure> {
    if let AgentRuntimeEvent::Context {
        used_tokens,
        window_tokens,
        cost_micros,
        ..
    } = event
    {
        if *used_tokens > 9_007_199_254_740_991
            || *window_tokens > 9_007_199_254_740_991
            || cost_micros.is_some_and(|value| value > 9_007_199_254_740_991)
        {
            return Err(invalid("invalid context counters"));
        }
    }
    Ok(())
}
