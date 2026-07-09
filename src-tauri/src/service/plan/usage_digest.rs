use crate::config::schema::UsageConfig;
use crate::usage::store::UsageSnapshotRow;

pub const FRESHNESS_MS: i64 = 180_000;
pub const HIGH_PCT_THRESHOLD: f32 = 80.0;

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
    let threshold = if cfg.high_pct_threshold > 0.0 {
        cfg.high_pct_threshold
    } else {
        HIGH_PCT_THRESHOLD
    };
    let mut lines = Vec::new();
    let mut high_labels = Vec::new();
    let mut low_labels = Vec::new();
    for row in rows {
        let line = render_usage_line(row, now_ms, cfg.freshness_secs.saturating_mul(1000) as i64);
        if row.high_pressure(threshold) {
            high_labels.push(row.label.clone());
        } else {
            low_labels.push(row.label.clone());
        }
        lines.push(line);
    }
    let guidance = balancing_guidance(&high_labels, &low_labels);
    (lines, guidance)
}

pub struct UsageDigestRow {
    pub label: String,
    pub row: Option<UsageSnapshotRow>,
}

impl UsageDigestRow {
    pub fn high_pressure(&self, threshold: f32) -> bool {
        self.row
            .as_ref()
            .and_then(primary_pct)
            .is_some_and(|pct| pct >= threshold)
    }
}

fn primary_pct(row: &UsageSnapshotRow) -> Option<f32> {
    row.weekly_pct.or(row.session_pct)
}

fn render_usage_line(row: &UsageDigestRow, now_ms: i64, freshness_ms: i64) -> String {
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
    if let Some(pct) = primary_pct(snapshot) {
        let window = if snapshot.weekly_pct.is_some() {
            "weekly"
        } else {
            "session"
        };
        let reset = snapshot
            .weekly_resets_at
            .or(snapshot.session_resets_at)
            .map(|ts| format_reset_local(ts))
            .unwrap_or_default();
        let flag = if pct >= HIGH_PCT_THRESHOLD {
            " — REDUCE"
        } else {
            ""
        };
        return format!(
            "{}: {:.0}% {window}{reset}{flag}{stale_tag}",
            row.label, pct
        );
    }
    if let (Some(spent), Some(limit)) = (snapshot.spend_cents, snapshot.spend_limit_cents) {
        let pct = if limit > 0.0 {
            (spent / limit) * 100.0
        } else {
            0.0
        };
        let flag = if pct >= HIGH_PCT_THRESHOLD as f64 {
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

fn format_reset_local(ts_ms: i64) -> String {
    use chrono::{TimeZone, Timelike, Utc};
    let Some(dt) = Utc.timestamp_millis_opt(ts_ms).single() else {
        return String::new();
    };
    let local = dt.with_timezone(&chrono::Local);
    format!(" (resets {:02}:{:02})", local.hour(), local.minute())
}

fn balancing_guidance(high: &[String], low: &[String]) -> Option<String> {
    if high.is_empty() || low.is_empty() {
        return None;
    }
    Some(format!(
        "{} >{:.0}%: prefer routing new dispatches to {} until reset",
        high.join("/"),
        HIGH_PCT_THRESHOLD,
        low.join("/")
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

    #[test]
    fn renders_provider_lines_and_balancing_guidance() {
        let cfg = UsageConfig::default();
        let now = 1_000_000_i64;
        let rows = vec![
            UsageDigestRow {
                label: "Codex".into(),
                row: Some(row("codex", 91.0, now - 30_000)),
            },
            UsageDigestRow {
                label: "Claude".into(),
                row: Some(row("claude", 62.0, now - 30_000)),
            },
        ];
        let (lines, guidance) = usage_digest_lines(&cfg, now, &rows);
        assert!(lines
            .iter()
            .any(|l| l.contains("91%") && l.contains("REDUCE")));
        assert!(lines.iter().any(|l| l.contains("62%")));
        let hint = guidance.expect("guidance");
        assert!(hint.contains("Codex"));
        assert!(hint.contains("Claude"));
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
