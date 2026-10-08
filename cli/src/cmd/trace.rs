use crate::args::trace::TraceAction;
use codeterm_types::trace::{gaps, read, recent, select};

fn since_ms(value: &str, now: u64) -> Result<u64, String> {
    let (number, factor) = if let Some(number) = value.strip_suffix('m') {
        (number, 60_000)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3_600_000)
    } else if let Some(number) = value.strip_suffix('d') {
        (number, 86_400_000)
    } else {
        return Err("--since requires a duration such as 30m, 1h or 2d".into());
    };
    let duration = number
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .and_then(|n| n.checked_mul(factor))
        .ok_or("invalid --since duration")?;
    Ok(now.saturating_sub(duration))
}

pub fn handle(json: bool, action: TraceAction) {
    let events = read(&crate::data_root().join("logs"))
        .unwrap_or_else(|error| crate::cmd::print::fail(1, error.to_string()));
    match action {
        TraceAction::Show { id, since } => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let since = since
                .map(|value| {
                    since_ms(&value, now).unwrap_or_else(|error| crate::cmd::print::fail(2, error))
                })
                .unwrap_or(0);
            let chain = select(&events, &id, since);
            let missing = gaps(&chain);
            if json {
                println!("{}", serde_json::json!({"events": chain, "gaps": missing}));
            } else {
                if chain.is_empty() {
                    println!("No retained trace events for {id}");
                }
                for event in chain {
                    println!(
                        "{} {} {} tab={} provider={} outcome={} {}",
                        event.ts,
                        event.trace_id,
                        event.stage,
                        event.tab,
                        event.provider,
                        event.outcome,
                        event.detail
                    );
                }
                for gap in missing {
                    println!("GAP: {gap}");
                }
            }
        }
        TraceAction::Recent { limit } => {
            let recent = recent(&events, limit as usize);
            if json {
                println!("{}", serde_json::json!({"flows": recent}));
            } else {
                for event in recent {
                    println!(
                        "{} {} tab={} {} {}",
                        event.ts, event.trace_id, event.tab, event.stage, event.outcome
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn since_rejects_invalid_and_overflowing_durations() {
        assert_eq!(since_ms("1h", 4_000_000).unwrap(), 400_000);
        for value in ["1", "0h", "-1m", "18446744073709551615d"] {
            assert!(since_ms(value, 4_000_000).is_err());
        }
    }
}
