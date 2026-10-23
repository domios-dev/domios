#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeadlineModel {
    pub(crate) duration_ms: Option<u64>,
    pub(crate) started_ms: Option<u64>,
    pub(crate) deadline_ms: Option<u64>,
}

impl DeadlineModel {
    pub(crate) fn set_timeout(&mut self, duration_ms: Option<u64>) -> Result<(), String> {
        self.deadline_ms = self
            .started_ms
            .zip(duration_ms)
            .map(|(start, duration)| deadline_from_start(start, duration))
            .transpose()?;
        self.duration_ms = duration_ms;
        Ok(())
    }

    pub(crate) fn command_started(&mut self, now_ms: u64) -> Result<(), String> {
        self.deadline_ms = self
            .duration_ms
            .map(|duration| deadline_from_start(now_ms, duration))
            .transpose()?;
        self.started_ms = Some(now_ms);
        Ok(())
    }

    pub(crate) fn command_finished(&mut self) {
        self.started_ms = None;
        self.deadline_ms = None;
    }

    pub(crate) fn expired(&self, now_ms: u64) -> bool {
        self.deadline_ms.is_some_and(|deadline| now_ms >= deadline)
    }
}

pub fn parse_timeout(value: &str) -> Result<Option<u64>, String> {
    if value == "none" {
        return Ok(None);
    }
    let (number, multiplier) = match value.as_bytes().last() {
        Some(b's') => (&value[..value.len() - 1], 1_000),
        Some(b'm') => (&value[..value.len() - 1], 60_000),
        Some(b'h') => (&value[..value.len() - 1], 3_600_000),
        _ => return Err("timeout requires a positive duration with s, m or h, or none".into()),
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(multiplier))
        .filter(|ms| *ms > 0 && *ms <= 365 * 24 * 3_600_000)
        .map(Some)
        .ok_or_else(|| "timeout must be positive and at most 365 days".into())
}

pub(crate) fn deadline_from_start(started_ms: u64, duration_ms: u64) -> Result<u64, String> {
    started_ms
        .checked_add(duration_ms)
        .ok_or_else(|| "deadline overflow".into())
}

pub(crate) fn elapsed_seconds(started_ms: u64, now_ms: u64) -> f64 {
    now_ms.saturating_sub(started_ms) as f64 / 1000.0
}

#[cfg(test)]
mod tests;
