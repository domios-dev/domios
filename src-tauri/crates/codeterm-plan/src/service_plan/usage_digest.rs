use codeterm_types::config::schema::UsageConfig;
use codeterm_types::usage::UsageSnapshotRow;

pub const FRESHNESS_MS: i64 = 180_000;
pub const HIGH_PCT_THRESHOLD: f32 = 90.0;

#[derive(Debug, Clone, PartialEq)]
pub struct UsageDigestLine {
    pub label: String,
    pub text: String,
    pub high_pressure: bool,
}

pub fn usage_digest_lines(
    cfg: &UsageConfig,
    now_ms: i64,
    rows: &[UsageDigestRow],
) -> (Vec<String>, Option<String>) {
    if !cfg.enabled || !cfg.heartbeat || rows.is_empty() {
        return (Vec::new(), None);
    }
    let mut lines = Vec::new();
    let mut high_count = 0;
    let mut lower_usage_count = 0;
    for row in rows {
        let line = render_usage_line(
            row,
            now_ms,
            cfg.freshness_secs.saturating_mul(1000) as i64,
            HIGH_PCT_THRESHOLD,
        );
        if row.high_pressure(HIGH_PCT_THRESHOLD) {
            high_count += 1;
        } else if row.pressure_pct().is_some() {
            lower_usage_count += 1;
        }
        lines.push(line);
    }
    let guidance = balancing_guidance(high_count, lower_usage_count, HIGH_PCT_THRESHOLD);
    (lines, guidance)
}

pub struct UsageDigestRow {
    pub label: String,
    pub row: Option<UsageSnapshotRow>,
}

impl UsageDigestRow {
    pub fn high_pressure(&self, threshold: f32) -> bool {
        self.pressure_pct().is_some_and(|pct| pct >= threshold)
    }

    fn pressure_pct(&self) -> Option<f32> {
        self.row.as_ref().and_then(pressure_pct)
    }
}

fn pressure_pct(row: &UsageSnapshotRow) -> Option<f32> {
    match (row.session_pct, row.weekly_pct) {
        (Some(session), Some(weekly)) => Some(session.max(weekly)),
        (Some(session), None) => Some(session),
        (None, Some(weekly)) => Some(weekly),
        (None, None) => None,
    }
}

fn render_usage_line(
    row: &UsageDigestRow,
    now_ms: i64,
    freshness_ms: i64,
    threshold: f32,
) -> String {
    let Some(snapshot) = row.row.as_ref() else {
        return format!("{}: no snapshot yet", row.label);
    };
    let age_ms = now_ms.saturating_sub(snapshot.captured_at);
    let stale = age_ms > freshness_ms.max(FRESHNESS_MS);
    let stale_tag = if stale {
        format!(" (stale {}m ago)", age_ms / 60_000)
    } else {
        String::new()
    };
    let mut windows = Vec::new();
    if let Some(pct) = snapshot.session_pct {
        windows.push(render_limit_window(
            pct,
            "session",
            snapshot.session_resets_at,
        ));
    }
    if let Some(pct) = snapshot.weekly_pct {
        windows.push(render_limit_window(
            pct,
            "weekly",
            snapshot.weekly_resets_at,
        ));
    }
    if !windows.is_empty() {
        let flag = if pressure_pct(snapshot).is_some_and(|pct| pct >= threshold) {
            " — REDUCE"
        } else {
            ""
        };
        return format!("{}: {}{flag}{stale_tag}", row.label, windows.join(", "));
    }
    if let (Some(spent), Some(limit)) = (snapshot.spend_cents, snapshot.spend_limit_cents) {
        let pct = if limit > 0.0 {
            (spent / limit) * 100.0
        } else {
            0.0
        };
        let flag = if pct >= threshold as f64 {
            " — REDUCE"
        } else {
            ""
        };
        return format!(
            "{}: ${:.0}/${:.0} spend{flag}{stale_tag}",
            row.label,
            spent / 100.0,
            limit / 100.0
        );
    }
    format!("{}: unavailable{stale_tag}", row.label)
}

fn render_limit_window(pct: f32, window: &str, resets_at: Option<i64>) -> String {
    let reset = resets_at.map(format_reset_local).unwrap_or_default();
    format!("{pct:.0}% {window}{reset}")
}

fn format_reset_local(ts_ms: i64) -> String {
    use chrono::{TimeZone, Timelike, Utc};
    let Some(dt) = Utc.timestamp_millis_opt(ts_ms).single() else {
        return String::new();
    };
    let local = dt.with_timezone(&chrono::Local);
    format!(" (resets {:02}:{:02})", local.hour(), local.minute())
}

fn balancing_guidance(
    high_count: usize,
    lower_usage_count: usize,
    threshold: f32,
) -> Option<String> {
    if high_count == 0 || lower_usage_count == 0 {
        return None;
    }
    Some(format!(
        "Usage pressure above {:.0}%: if further dispatch is needed, use available lower-usage workers until reset",
        threshold
    ))
}

pub fn needs_refresh(row: Option<&UsageSnapshotRow>, now_ms: i64, freshness_ms: i64) -> bool {
    match row {
        None => true,
        Some(snapshot) => {
            now_ms.saturating_sub(snapshot.captured_at) > freshness_ms.max(FRESHNESS_MS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(provider: &str, weekly: f32, captured_at: i64) -> UsageSnapshotRow {
        UsageSnapshotRow {
            id: 1,
            provider: provider.into(),
            account_id: None,
            captured_at,
            parsed_ok: true,
            session_pct: None,
            session_resets_at: None,
            weekly_pct: Some(weekly),
            weekly_resets_at: Some(captured_at + 3_600_000),
            spend_cents: None,
            spend_limit_cents: None,
            spend_remaining_cents: None,
            spend_resets_at: None,
            failure_reason: None,
            probe_command: None,
            raw_excerpt: None,
            debug_json: None,
        }
    }

    fn local_timestamp(hour: u32, minute: u32) -> i64 {
        use chrono::TimeZone;
        chrono::Local
            .with_ymd_and_hms(2027, 1, 15, hour, minute, 0)
            .single()
            .expect("test local timestamp")
            .timestamp_millis()
    }

    #[test]
    fn renders_session_and_weekly_windows_when_both_are_available() {
        let cfg = UsageConfig::default();
        let now = 1_800_000_000_000_i64;
        let mut snapshot = row("claude", 21.0, now);
        snapshot.session_pct = Some(37.0);
        snapshot.session_resets_at = Some(local_timestamp(7, 8));
        snapshot.weekly_resets_at = Some(local_timestamp(9, 10));
        let rows = vec![UsageDigestRow {
            label: "Claude Code".into(),
            row: Some(snapshot),
        }];

        let (lines, guidance) = usage_digest_lines(&cfg, now, &rows);

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            "Claude Code: 37% session (resets 07:08), 21% weekly (resets 09:10)"
        );
        assert!(guidance.is_none());
    }

    #[test]
    fn missing_session_data_keeps_the_existing_weekly_only_wording() {
        let cfg = UsageConfig::default();
        let now = 1_800_000_000_000_i64;
        let mut snapshot = row("codex", 7.0, now);
        snapshot.session_resets_at = Some(local_timestamp(7, 8));
        snapshot.weekly_resets_at = Some(local_timestamp(9, 10));
        let rows = vec![UsageDigestRow {
            label: "Codex".into(),
            row: Some(snapshot),
        }];

        let (lines, guidance) = usage_digest_lines(&cfg, now, &rows);

        assert_eq!(lines[0], "Codex: 7% weekly (resets 09:10)");
        assert!(!lines[0].contains("session"));
        assert!(guidance.is_none());
    }

    #[test]
    fn default_threshold_is_ninety_and_guidance_is_provider_neutral() {
        let cfg = UsageConfig::default();
        let now = 1_000_000_i64;
        let rows = vec![
            UsageDigestRow {
                label: "Codex".into(),
                row: Some(row("codex", 91.0, now - 30_000)),
            },
            UsageDigestRow {
                label: "Claude Code".into(),
                row: Some(row("claude", 62.0, now - 30_000)),
            },
        ];
        let (lines, guidance) = usage_digest_lines(&cfg, now, &rows);
        assert!(lines
            .iter()
            .any(|l| l.contains("91%") && l.contains("REDUCE")));
        assert!(lines.iter().any(|l| l.contains("62%")));
        let hint = guidance.expect("guidance");
        assert!(hint.contains("90%"));
        assert!(hint.contains("available lower-usage workers"));
        assert!(!hint.contains("Codex"));
        assert!(!hint.contains("Claude Code"));
    }

    #[test]
    fn fixed_threshold_does_not_reduce_the_ninety_percent_policy() {
        let cfg = UsageConfig::default();
        let now = 1_000_000_i64;
        let rows = vec![
            UsageDigestRow {
                label: "Provider A".into(),
                row: Some(row("a", 89.0, now)),
            },
            UsageDigestRow {
                label: "Provider B".into(),
                row: Some(row("b", 91.0, now)),
            },
        ];
        let (lines, guidance) = usage_digest_lines(&cfg, now, &rows);
        assert!(!lines[0].contains("REDUCE"));
        assert!(lines[1].contains("REDUCE"));
        assert!(guidance.expect("guidance").contains("90%"));
    }

    #[test]
    fn does_not_offer_guidance_without_known_lower_usage_provider() {
        let cfg = UsageConfig::default();
        let now = 1_000_000_i64;
        let all_high = vec![
            UsageDigestRow {
                label: "Provider A".into(),
                row: Some(row("a", 91.0, now)),
            },
            UsageDigestRow {
                label: "Provider B".into(),
                row: Some(row("b", 95.0, now)),
            },
        ];
        assert!(usage_digest_lines(&cfg, now, &all_high).1.is_none());

        let no_lower_snapshot = vec![
            UsageDigestRow {
                label: "Provider A".into(),
                row: Some(row("a", 91.0, now)),
            },
            UsageDigestRow {
                label: "Provider B".into(),
                row: None,
            },
        ];
        assert!(usage_digest_lines(&cfg, now, &no_lower_snapshot)
            .1
            .is_none());
    }

    #[test]
    fn stale_snapshots_carry_timestamp_without_blocking() {
        let cfg = UsageConfig::default();
        let now = 1_000_000_i64;
        let rows = vec![UsageDigestRow {
            label: "Claude".into(),
            row: Some(row("claude", 40.0, now - 600_000)),
        }];
        let (lines, guidance) = usage_digest_lines(&cfg, now, &rows);
        assert!(lines[0].contains("stale"));
        assert!(guidance.is_none());
    }

    #[test]
    fn needs_refresh_when_missing_or_old() {
        let now = 500_000;
        assert!(needs_refresh(None, now, FRESHNESS_MS));
        assert!(!needs_refresh(
            Some(&row("claude", 1.0, now - 60_000)),
            now,
            FRESHNESS_MS
        ));
        assert!(needs_refresh(
            Some(&row("claude", 1.0, now - 400_000)),
            now,
            FRESHNESS_MS
        ));
    }
}
