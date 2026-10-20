pub const CODEX_EXTREME_NEIGHBOR_WINDOW_MS: i64 = 2 * 60 * 60 * 1000;
pub const RAW_EXCERPT_MAX_CHARS: usize = 4_000;
pub const CODEX_ZERO_ZERO_BACKWARD_RESETS: &str =
    "suspicious codex usage snapshot: 0%/0% with reset timestamps older than the previous accepted sample";
pub const CODEX_HUNDRED_HUNDRED_IDENTICAL_RESETS: &str =
    "suspicious codex usage snapshot: 100%/100% with identical session and weekly reset timestamps";

#[derive(Debug, Clone, Copy)]
pub struct UsageGuardSnapshot {
    pub captured_at_ms: i64,
    pub session_pct: Option<f32>,
    pub session_resets_at_ms: Option<i64>,
    pub weekly_pct: Option<f32>,
    pub weekly_resets_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
pub struct AcceptedUsageSnapshot {
    pub captured_at_ms: i64,
    pub session_resets_at_ms: Option<i64>,
    pub weekly_resets_at_ms: Option<i64>,
}

pub fn suspicious_codex_reason(
    snap: UsageGuardSnapshot,
    previous: Option<AcceptedUsageSnapshot>,
) -> Option<&'static str> {
    let session_pct = snap.session_pct?;
    let weekly_pct = snap.weekly_pct?;
    let both_zero = session_pct <= 0.0 && weekly_pct <= 0.0;
    let both_full = session_pct >= 100.0 && weekly_pct >= 100.0;
    if !both_zero && !both_full {
        return None;
    }

    let prev = previous?;
    if snap.captured_at_ms - prev.captured_at_ms > CODEX_EXTREME_NEIGHBOR_WINDOW_MS {
        return None;
    }

    if both_zero && resets_move_backwards(snap, prev) {
        return Some(CODEX_ZERO_ZERO_BACKWARD_RESETS);
    }
    if both_full && identical_session_weekly_reset(snap) && !identical_previous_reset(prev) {
        return Some(CODEX_HUNDRED_HUNDRED_IDENTICAL_RESETS);
    }

    None
}

pub fn rejected_debug_json(reason: &str, snap: UsageGuardSnapshot, raw_text: &str) -> String {
    format!(
        "{{\"reason\":{},\"captured_at_ms\":{},\"session_pct\":{},\"session_resets_at_ms\":{},\"weekly_pct\":{},\"weekly_resets_at_ms\":{},\"raw_excerpt_chars\":{}}}",
        json_string(reason),
        snap.captured_at_ms,
        json_f32(snap.session_pct),
        json_i64(snap.session_resets_at_ms),
        json_f32(snap.weekly_pct),
        json_i64(snap.weekly_resets_at_ms),
        bounded_raw_excerpt(raw_text).chars().count(),
    )
}

pub fn bounded_raw_excerpt(raw: &str) -> String {
    raw.chars().take(RAW_EXCERPT_MAX_CHARS).collect()
}

fn resets_move_backwards(snap: UsageGuardSnapshot, prev: AcceptedUsageSnapshot) -> bool {
    reset_before(snap.session_resets_at_ms, prev.session_resets_at_ms)
        || reset_before(snap.weekly_resets_at_ms, prev.weekly_resets_at_ms)
}

fn reset_before(current: Option<i64>, previous: Option<i64>) -> bool {
    matches!((current, previous), (Some(current), Some(previous)) if current < previous)
}

fn identical_session_weekly_reset(snap: UsageGuardSnapshot) -> bool {
    matches!(
        (snap.session_resets_at_ms, snap.weekly_resets_at_ms),
        (Some(session), Some(weekly)) if session == weekly
    )
}

fn identical_previous_reset(prev: AcceptedUsageSnapshot) -> bool {
    matches!(
        (prev.session_resets_at_ms, prev.weekly_resets_at_ms),
        (Some(session), Some(weekly)) if session == weekly
    )
}

fn json_f32(value: Option<f32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_i64(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(
        captured_at_ms: i64,
        session_pct: f32,
        weekly_pct: f32,
        session_reset: i64,
        weekly_reset: i64,
    ) -> UsageGuardSnapshot {
        UsageGuardSnapshot {
            captured_at_ms,
            session_pct: Some(session_pct),
            session_resets_at_ms: Some(session_reset),
            weekly_pct: Some(weekly_pct),
            weekly_resets_at_ms: Some(weekly_reset),
        }
    }

    fn prev(captured_at_ms: i64, session_reset: i64, weekly_reset: i64) -> AcceptedUsageSnapshot {
        AcceptedUsageSnapshot {
            captured_at_ms,
            session_resets_at_ms: Some(session_reset),
            weekly_resets_at_ms: Some(weekly_reset),
        }
    }

    #[test]
    fn implausible_codex_zero_zero_backward_reset_rejected_with_evidence() {
        let candidate = snap(
            1_783_601_095_000,
            0.0,
            0.0,
            1_783_608_240_000,
            1_784_132_760_000,
        );
        let previous = prev(1_783_601_000_000, 1_783_609_680_000, 1_784_139_960_000);

        let reason = suspicious_codex_reason(candidate, Some(previous)).unwrap();
        assert_eq!(reason, CODEX_ZERO_ZERO_BACKWARD_RESETS);
        let raw = format!("OpenAI Codex\n{}", "x".repeat(RAW_EXCERPT_MAX_CHARS + 20));
        let excerpt = bounded_raw_excerpt(&raw);
        let debug = rejected_debug_json(reason, candidate, &raw);

        assert_eq!(excerpt.chars().count(), RAW_EXCERPT_MAX_CHARS);
        assert!(excerpt.contains("OpenAI Codex"));
        assert!(debug.contains("0%/0%"));
        assert!(debug.contains("\"raw_excerpt_chars\":4000"));
    }

    #[test]
    fn implausible_codex_hundred_hundred_identical_reset_rejected() {
        let candidate = snap(
            1_783_601_095_000,
            100.0,
            100.0,
            1_783_700_000_000,
            1_783_700_000_000,
        );
        let previous = prev(1_783_601_000_000, 1_783_609_680_000, 1_784_139_960_000);

        assert_eq!(
            suspicious_codex_reason(candidate, Some(previous)),
            Some(CODEX_HUNDRED_HUNDRED_IDENTICAL_RESETS)
        );
    }

    #[test]
    fn supported_reset_and_exhaustion_samples_are_accepted() {
        let reset = snap(
            1_783_610_000_000,
            0.0,
            9.0,
            1_783_628_000_000,
            1_784_139_960_000,
        );
        let exhausted = snap(
            1_783_601_095_000,
            100.0,
            100.0,
            1_783_609_680_000,
            1_784_139_960_000,
        );
        let previous = prev(1_783_601_000_000, 1_783_609_680_000, 1_784_139_960_000);

        assert_eq!(suspicious_codex_reason(reset, Some(previous)), None);
        assert_eq!(suspicious_codex_reason(exhausted, Some(previous)), None);
    }
}
