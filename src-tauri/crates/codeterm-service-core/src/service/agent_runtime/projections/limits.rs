use codeterm_plugin::{
    agent_runtime::{validate_rate_limits, RuntimeFailure, RuntimeRateLimit},
    api_contracts::Reading,
};

pub(super) fn merge(
    previous: Option<Reading<Vec<RuntimeRateLimit>>>,
    incoming: Vec<RuntimeRateLimit>,
    at: u64,
) -> Result<Reading<Vec<RuntimeRateLimit>>, RuntimeFailure> {
    let reported = incoming
        .iter()
        .map(|window| window.window.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut merged = previous.unwrap_or(Reading {
        captured_at: at,
        value: vec![],
    });
    if at >= merged.captured_at {
        for mut window in incoming {
            match merged
                .value
                .iter_mut()
                .find(|held| held.window == window.window)
            {
                Some(held) => {
                    window.resets_at = window.resets_at.or(held.resets_at);
                    window.window_minutes = window.window_minutes.or(held.window_minutes);
                    *held = window;
                }
                None => merged.value.push(window),
            }
        }
        merged.captured_at = at;
    }
    expire(&mut merged, at);
    if validate_rate_limits(&merged.value).is_err() {
        merged
            .value
            .retain(|window| window.resets_at.is_some() || reported.contains(&window.window));
    }
    validate_rate_limits(&merged.value)?;
    Ok(merged)
}

pub(super) fn expire(reading: &mut Reading<Vec<RuntimeRateLimit>>, now: u64) {
    for window in &mut reading.value {
        window.resets_at = window.resets_at.filter(|reset| *reset > now);
    }
}

#[cfg(test)]
mod tests;
