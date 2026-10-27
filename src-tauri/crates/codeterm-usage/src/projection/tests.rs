use super::*;
use crate::store::UsageStore;
use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset};

#[derive(Clone, Copy, Debug)]
struct TransitionZone {
    at_ms: i64,
    before: i32,
    after: i32,
}

#[derive(Clone, Debug)]
struct TransitionOffset {
    zone: TransitionZone,
    seconds: i32,
}

impl std::fmt::Display for TransitionOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.fix(), f)
    }
}

impl Offset for TransitionOffset {
    fn fix(&self) -> FixedOffset {
        FixedOffset::east_opt(self.seconds).unwrap()
    }
}

impl TimeZone for TransitionZone {
    type Offset = TransitionOffset;

    fn from_offset(offset: &Self::Offset) -> Self {
        offset.zone
    }

    fn offset_from_local_date(&self, date: &NaiveDate) -> LocalResult<Self::Offset> {
        self.offset_from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<Self::Offset> {
        let candidates: Vec<_> = [self.before, self.after]
            .into_iter()
            .filter(|seconds| {
                let utc = *local - chrono::Duration::seconds(i64::from(*seconds));
                self.offset_from_utc_datetime(&utc).seconds == *seconds
            })
            .map(|seconds| TransitionOffset {
                zone: *self,
                seconds,
            })
            .collect();
        match candidates.as_slice() {
            [] => LocalResult::None,
            [one] => LocalResult::Single(one.clone()),
            [a, b] => LocalResult::Ambiguous(a.clone(), b.clone()),
            _ => unreachable!(),
        }
    }

    fn offset_from_utc_date(&self, date: &NaiveDate) -> Self::Offset {
        self.offset_from_utc_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> Self::Offset {
        TransitionOffset {
            zone: *self,
            seconds: if utc.and_utc().timestamp_millis() < self.at_ms {
                self.before
            } else {
                self.after
            },
        }
    }
}

fn utc(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp_millis()
}

fn date(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

fn riga_spring() -> Calendar<TransitionZone> {
    Calendar(TransitionZone {
        at_ms: utc("2026-03-29T01:00:00Z"),
        before: 7200,
        after: 10800,
    })
}

fn riga_fall() -> Calendar<TransitionZone> {
    Calendar(TransitionZone {
        at_ms: utc("2026-10-25T01:00:00Z"),
        before: 10800,
        after: 7200,
    })
}

const RESETS_A: i64 = 9_000_000_000_000;
const RESETS_B: i64 = RESETS_A + 7 * 24 * 3_600_000;

// Unique provider per test to avoid the static BASELINE_CACHE bleeding between parallel test threads.
fn unique_provider(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("test-{}-{}", tag, n)
}

fn store() -> UsageStoreHandle {
    UsageStore::open_in_memory().unwrap()
}

fn insert(
    s: &UsageStoreHandle,
    provider: &str,
    captured_at: i64,
    weekly_pct: Option<f64>,
    weekly_resets_at: Option<i64>,
) {
    s.with_conn(|c| {
        c.execute(
            "INSERT INTO usage_snapshots (provider, account_id, captured_at, parsed_ok, \
             session_pct, session_resets_at, weekly_pct, weekly_resets_at) \
             VALUES (?1, NULL, ?2, 1, NULL, NULL, ?3, ?4)",
            rusqlite::params![provider, captured_at, weekly_pct, weekly_resets_at],
        )
        .unwrap();
    });
}

fn now_at_hour(hour: u32) -> i64 {
    let base = Local
        .with_ymd_and_hms(2026, 1, 15, 0, 0, 0)
        .earliest()
        .unwrap()
        .timestamp_millis();
    base + (hour as i64) * 3_600_000
}

#[test]
fn empty_db_returns_no_weekly() {
    let p = unique_provider("empty");
    let s = store();
    let r = compute(&s, &p, None, now_at_hour(15)).unwrap();
    assert!(r.weekly.is_none());
    assert!(r.session.is_none());
}

#[test]
fn no_finite_projection_when_share_zero_and_no_activity() {
    let p = unique_provider("share-zero");
    let s = store();
    let midnight = now_at_hour(0);
    insert(&s, &p, midnight, Some(0.0), Some(RESETS_A));
    let r = compute(&s, &p, None, midnight + 1000).unwrap();
    let w = r.weekly.unwrap();
    assert!(w.end_of_today_pct.is_finite());
    assert!(w.end_of_today_pct >= 0.0);
    assert!(w.end_of_cycle_pct.is_finite());
    assert!(w.rest_of_today_est >= 0.0);
}

#[test]
fn cycle_reset_overnight_uses_intra_day_delta() {
    let p = unique_provider("reset-overnight");
    let s = store();
    let now = now_at_hour(15);
    let yest = now - DAY_MS;
    insert(&s, &p, yest, Some(95.0), Some(RESETS_A));
    insert(&s, &p, now - 3_600_000, Some(10.0), Some(RESETS_B));
    insert(&s, &p, now, Some(20.0), Some(RESETS_B));
    let delta = today_delta(&s, &p, None, now, 20.0, RESETS_B).unwrap();
    assert!((delta - 10.0).abs() < 0.01, "got {}", delta);
}

#[test]
fn cross_day_no_reset_uses_yest_delta() {
    let p = unique_provider("cross-day");
    let s = store();
    let now = now_at_hour(15);
    let yest = now - 6 * 3_600_000;
    insert(&s, &p, yest, Some(6.0), Some(RESETS_A));
    insert(&s, &p, now - 3_600_000, Some(28.0), Some(RESETS_A));
    insert(&s, &p, now, Some(34.0), Some(RESETS_A));
    let delta = today_delta(&s, &p, None, now, 34.0, RESETS_A).unwrap();
    assert!((delta - 28.0).abs() < 0.01, "got {}", delta);
}

#[test]
fn today_delta_no_data_is_zero() {
    let p = unique_provider("td-empty");
    let s = store();
    let now = now_at_hour(15);
    let delta = today_delta(&s, &p, None, now, 0.0, RESETS_A).unwrap();
    assert_eq!(delta, 0.0);
}

#[test]
fn recent_rate_zero_when_single_sample() {
    let p = unique_provider("rate-single");
    let s = store();
    let now = now_at_hour(15);
    insert(&s, &p, now, Some(10.0), Some(RESETS_A));
    let r = recent_rate(&s, &p, None, now).unwrap();
    assert_eq!(r, 0.0);
}

#[test]
fn recent_rate_zero_when_zero_dt() {
    let p = unique_provider("rate-zerodt");
    let s = store();
    let now = now_at_hour(15);
    insert(&s, &p, now, Some(10.0), Some(RESETS_A));
    insert(&s, &p, now, Some(11.0), Some(RESETS_A));
    let r = recent_rate(&s, &p, None, now).unwrap();
    assert_eq!(r, 0.0);
}

#[test]
fn baseline_shape_sums_to_one() {
    let p = unique_provider("shape-sum");
    let s = store();
    let today_start = now_at_hour(0);
    for d in 1..=14 {
        let day = today_start - d * DAY_MS;
        insert(&s, &p, day + 13 * 3_600_000, Some(0.0), Some(RESETS_A));
        insert(&s, &p, day + 15 * 3_600_000, Some(20.0), Some(RESETS_A));
    }
    let b = compute_baseline(&s, &p, None, today_start + 15 * 3_600_000).unwrap();
    let wd_sum: f64 = b.wd_hour_share.iter().sum();
    let we_sum: f64 = b.we_hour_share.iter().sum();
    assert!((wd_sum - 1.0).abs() < 0.001, "wd shape sum = {}", wd_sum);
    assert!((we_sum - 1.0).abs() < 0.001, "we shape sum = {}", we_sum);
}

#[test]
fn ratio_clamped_to_two_when_we_far_exceeds_wd() {
    let p = unique_provider("ratio-clamp");
    let s = store();
    let today_start = now_at_hour(0);
    for d in 1..=14 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        let delta = if dow == 0 || dow == 6 { 200.0 } else { 1.0 };
        insert(&s, &p, day + 12 * 3_600_000, Some(0.0), Some(RESETS_A));
        insert(&s, &p, day + 14 * 3_600_000, Some(delta), Some(RESETS_A));
    }
    let b = compute_baseline(&s, &p, None, today_start + 12 * 3_600_000).unwrap();
    assert!(
        b.we_to_wd_ratio <= 2.0 + 1e-9,
        "ratio = {}",
        b.we_to_wd_ratio
    );
    assert!(b.we_to_wd_ratio >= 0.02 - 1e-9);
}

#[test]
fn ratio_default_when_no_weekday_history() {
    let p = unique_provider("ratio-default");
    let s = store();
    let now = now_at_hour(15);
    let b = compute_baseline(&s, &p, None, now).unwrap();
    assert!(b.we_to_wd_ratio > 0.0);
    assert!(b.we_to_wd_ratio.is_finite());
}

#[test]
fn end_of_cycle_caps_at_200() {
    let p = unique_provider("cap-200");
    let s = store();
    let now = now_at_hour(23);
    let cycle_end = now + 7 * DAY_MS;
    insert(&s, &p, now, Some(190.0), Some(cycle_end));
    let r = compute(&s, &p, None, now).unwrap();
    let w = r.weekly.unwrap();
    assert!(w.end_of_today_pct <= 200.0);
    assert!(w.end_of_cycle_pct <= 200.0);
}

#[test]
fn cache_invalidate_forces_recompute() {
    let p = unique_provider("cache");
    let s = store();
    let now = now_at_hour(15);
    let today_start = local_start_of_day_ms(now);
    // Seed multiple weekdays so wd_total_avg comes from data, not fallback.
    let mut weekdays_seeded = 0;
    for d in 1..=21 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        if dow == 0 || dow == 6 {
            continue;
        }
        insert(&s, &p, day + 13 * 3_600_000, Some(0.0), Some(RESETS_A));
        insert(&s, &p, day + 15 * 3_600_000, Some(15.0), Some(RESETS_A));
        weekdays_seeded += 1;
        if weekdays_seeded >= 5 {
            break;
        }
    }
    let b1 = baseline_for(&s, &p, None, now).unwrap();
    assert!(b1.wd_total_avg > 0.0);
    let b1_dow_n_sum: u32 = b1.dow_n.iter().sum();
    // Add many more weekday samples — should NOT appear until we invalidate.
    for d in 22..32 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        if dow == 0 || dow == 6 {
            continue;
        }
        insert(&s, &p, day + 13 * 3_600_000, Some(0.0), Some(RESETS_A));
        insert(&s, &p, day + 15 * 3_600_000, Some(15.0), Some(RESETS_A));
    }
    let b_cached = baseline_for(&s, &p, None, now).unwrap();
    let b_cached_dow_n_sum: u32 = b_cached.dow_n.iter().sum();
    assert_eq!(
        b_cached_dow_n_sum, b1_dow_n_sum,
        "cached should not see new rows"
    );
    invalidate_cache(&p, None);
    let b2 = baseline_for(&s, &p, None, now).unwrap();
    let b2_dow_n_sum: u32 = b2.dow_n.iter().sum();
    assert!(
        b2_dow_n_sum > b1_dow_n_sum,
        "fresh baseline should see new rows"
    );
}

#[test]
fn shape_skips_offdevice_jump_pair() {
    // must NOT pin the 35% to a single hour — that pair gets dropped from per-hour aggregation entirely.
    let p = unique_provider("offdev");
    let s = store();
    let today_start = now_at_hour(0);
    // Need ≥3 days for confidence>=Medium; replicate across 4 weekdays.
    let mut wd_days_seeded = 0;
    for d in 1..=14 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        if dow == 0 || dow == 6 {
            continue;
        }
        insert(&s, &p, day + 9 * 3_600_000, Some(0.0), Some(RESETS_A));
        insert(&s, &p, day + 10 * 3_600_000, Some(4.0), Some(RESETS_A));
        insert(&s, &p, day + 11 * 3_600_000, Some(8.0), Some(RESETS_A));
        insert(&s, &p, day + 12 * 3_600_000, Some(12.0), Some(RESETS_A));
        insert(&s, &p, day + 13 * 3_600_000, Some(16.0), Some(RESETS_A));
        // Off-device jump from 13 → 19 (6h gap, +35%)
        insert(&s, &p, day + 19 * 3_600_000, Some(51.0), Some(RESETS_A));
        wd_days_seeded += 1;
        if wd_days_seeded >= 5 {
            break;
        }
    }
    let b = compute_baseline(&s, &p, None, today_start + 12 * 3_600_000).unwrap();
    // Hour 19 should NOT be a giant spike — the jump was rejected.
    let h19_share = b.wd_hour_share[19];
    let h10_share = b.wd_hour_share[10];
    assert!(
        h19_share < h10_share * 2.0,
        "expected 19h share ({}) to NOT explode vs 10h ({})",
        h19_share,
        h10_share,
    );
    // Shape still sums to 1.
    let sum: f64 = b.wd_hour_share.iter().sum();
    assert!((sum - 1.0).abs() < 0.01, "shape sum {}", sum);
}

#[test]
fn median_resists_vacation_outlier() {
    // sampling: 8 samples through the day (~4% per pair).
    let p = unique_provider("vac");
    let s = store();
    let today_start = now_at_hour(0);
    let hours_of_day = [9, 10, 11, 12, 14, 15, 16, 17];
    let mut mondays = 0;
    for d in 1..=84 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        if dow != 1 {
            continue;
        }
        mondays += 1;
        let total = if mondays == 3 { 4.0 } else { 28.0 };
        let n = hours_of_day.len();
        for (i, hr) in hours_of_day.iter().enumerate() {
            let pct = total * (i as f64) / (n as f64 - 1.0);
            insert(&s, &p, day + hr * 3_600_000, Some(pct), Some(RESETS_A));
        }
        if mondays >= 11 {
            break;
        }
    }
    let b = compute_baseline(&s, &p, None, today_start + 12 * 3_600_000).unwrap();
    let mon = b.dow_total[1];
    assert!(
        (mon - 28.0).abs() < 5.0,
        "expected Mon median ≈ 28%, got {} (mean would be ~26%)",
        mon
    );
}

#[test]
fn compute_handles_all_nans_gracefully() {
    let p = unique_provider("nan");
    let s = store();
    let now = now_at_hour(15);
    insert(&s, &p, now, Some(0.0), Some(RESETS_A));
    let r = compute(&s, &p, None, now).unwrap();
    let w = r.weekly.unwrap();
    assert!(w.end_of_today_pct.is_finite());
    assert!(w.end_of_cycle_pct.is_finite());
    assert!(w.avg_weekday_spend.is_finite());
    assert!(w.avg_weekend_spend.is_finite());
    assert!(w.rest_of_today_est.is_finite());
    assert!(w.rate_per_min_recent.is_finite());
}

// ── Gap-aware today_delta tests ────────────────────────────────────────────

/// Pre-midnight reading close to midnight (<2h gap): cross-midnight delta is
/// used (the probe ran near end-of-day so the gap is attributable to today).
#[test]
fn from_yest_used_when_near_midnight() {
    let p = unique_provider("yest-near");
    let s = store();
    let now = now_at_hour(10);
    let today_start_ms = local_start_of_day_ms(now);
    let near_midnight = today_start_ms - 60 * 60 * 1000;
    insert(&s, &p, near_midnight, Some(30.0), Some(RESETS_A));
    insert(&s, &p, now, Some(40.0), Some(RESETS_A));
    let delta = today_delta(&s, &p, None, now, 40.0, RESETS_A).unwrap();
    assert!((delta - 10.0).abs() < 0.01, "got {}", delta);
}

/// Pre-midnight reading far from midnight (>2h gap): cross-midnight delta is
/// discarded. Gap spending is not credited to today — we fall back to
/// from_today_min (which is 0 when today has only one reading).
#[test]
fn from_yest_ignored_when_gap_too_large() {
    let p = unique_provider("yest-far");
    let s = store();
    let now = now_at_hour(10);
    let today_start_ms = local_start_of_day_ms(now);
    let far_before_midnight = today_start_ms - 10 * 60 * 60 * 1000;
    insert(&s, &p, far_before_midnight, Some(34.0), Some(RESETS_A));
    insert(&s, &p, now, Some(50.0), Some(RESETS_A));
    let delta = today_delta(&s, &p, None, now, 50.0, RESETS_A).unwrap();
    // The 16% gap delta must NOT appear as "spent today"
    assert!((delta - 0.0).abs() < 0.01, "expected 0, got {}", delta);
}

/// Mid-day cycle reset: snapshot drops mid-day from 98% → 0% (manual reset /
/// plan upgrade). The pair-walk skips the negative jump, attributing only the
/// post-reset climb to the day — not the bogus `MAX - MIN = 98%` MIN-fallback
/// would've returned.
#[test]
fn mid_day_reset_skipped_in_pair_walk() {
    let p = unique_provider("mid-day-reset");
    let s = store();
    let today_start = now_at_hour(0);
    insert(&s, &p, today_start + 3_600_000, Some(95.0), Some(RESETS_A));
    insert(&s, &p, today_start + 5_400_000, Some(98.0), Some(RESETS_A));
    insert(&s, &p, today_start + 7_200_000, Some(0.0), Some(RESETS_A));
    insert(&s, &p, today_start + 10_800_000, Some(5.0), Some(RESETS_A));
    insert(&s, &p, today_start + 14_400_000, Some(8.0), Some(RESETS_A));
    let now = today_start + 14_400_000 + 1;
    let delta = today_delta(&s, &p, None, now, 8.0, RESETS_A).unwrap();
    // Pre-reset 95→98 = +3, post-reset 0→5→8 = +8. Negative 98→0 dropped.
    assert!((delta - 11.0).abs() < 0.01, "expected 11, got {}", delta);
}

/// Same large-gap scenario but with multiple readings today: the intra-day
/// min is used correctly regardless of the pre-midnight gap.
#[test]
fn from_yest_gap_with_intra_day_readings() {
    let p = unique_provider("yest-far-intra");
    let s = store();
    let now = now_at_hour(14);
    let today_start_ms = local_start_of_day_ms(now);
    let far_before_midnight = today_start_ms - 10 * 60 * 60 * 1000;
    insert(&s, &p, far_before_midnight, Some(34.0), Some(RESETS_A));
    insert(&s, &p, now_at_hour(10), Some(50.0), Some(RESETS_A));
    insert(&s, &p, now, Some(58.0), Some(RESETS_A));
    let delta = today_delta(&s, &p, None, now, 58.0, RESETS_A).unwrap();
    // from_today_min = 58 - 50 = 8 (genuine intra-day activity)
    assert!((delta - 8.0).abs() < 0.01, "expected 8, got {}", delta);
}

// ── compute_day_actuals (per-cycle daily deltas) ───────────────────────────

#[test]
fn day_actuals_emits_one_bucket_per_cycle_day() {
    let p = unique_provider("actuals-len");
    let s = store();
    let now = now_at_hour(15);
    let cycle_end = now + 5 * DAY_MS;
    let actuals = compute_day_actuals(&s, &p, None, cycle_end, &Calendar(Local)).unwrap();
    // 7 days back from cycle_end, aligned to local-start-of-day → at least 7, possibly 8 if the cycle's start time crossed a midnight boundary.
    assert!(
        (7..=8).contains(&actuals.len()),
        "got {} buckets",
        actuals.len()
    );
    assert!(
        actuals.iter().all(|a| a.pct.is_none()),
        "empty db → all None"
    );
}

#[test]
fn day_actuals_sparse_history_attributes_only_observed_spend() {
    // Cycle started 7 days ago. Only days 3–today have polling data. The first two empty days must stay None (not inherit cumulative).
    let p = unique_provider("actuals-sparse");
    let s = store();
    let now = now_at_hour(15);
    let today_start = local_start_of_day_ms(now);
    let cycle_end = today_start + DAY_MS;
    insert(
        &s,
        &p,
        today_start - 4 * DAY_MS + 9 * 3_600_000,
        Some(5.0),
        Some(cycle_end),
    );
    insert(
        &s,
        &p,
        today_start - 4 * DAY_MS + 15 * 3_600_000,
        Some(10.0),
        Some(cycle_end),
    );
    insert(
        &s,
        &p,
        today_start - 3 * DAY_MS + 10 * 3_600_000,
        Some(12.0),
        Some(cycle_end),
    );
    insert(
        &s,
        &p,
        today_start - 3 * DAY_MS + 18 * 3_600_000,
        Some(14.0),
        Some(cycle_end),
    );
    // Today: 14 (last yesterday EOD-ish) → 22 now. yest 22:00 = 14.
    insert(
        &s,
        &p,
        today_start - 2 * 3_600_000,
        Some(14.0),
        Some(cycle_end),
    );
    insert(&s, &p, now, Some(22.0), Some(cycle_end));
    let actuals = compute_day_actuals(&s, &p, None, cycle_end, &Calendar(Local)).unwrap();
    // Today bucket is the LAST one (day_start_ms == today_start).
    let today_bucket = actuals
        .iter()
        .find(|a| a.day_start_ms == today_start)
        .unwrap();
    // today has 1 sample. Bridge 14→22 (within 2h gap) = +8. Total 8.
    assert!(today_bucket.pct.is_some());
    let v = today_bucket.pct.unwrap();
    assert!((v - 8.0).abs() < 0.01, "today bucket got {}", v);
    // Days with no samples should be None, not 0.
    let empty_days = actuals.iter().filter(|a| a.pct.is_none()).count();
    assert!(
        empty_days >= 2,
        "expected ≥2 empty days, got {}",
        empty_days
    );
}

#[test]
fn day_actuals_skip_other_cycle_rows() {
    // A snapshot from a different cycle (RESETS_B) appearing within this cycle's day window must not contribute to that day's delta.
    let p = unique_provider("actuals-cycle-filter");
    let s = store();
    let now = now_at_hour(15);
    let today_start = local_start_of_day_ms(now);
    let cycle_end = today_start + DAY_MS;
    insert(
        &s,
        &p,
        today_start - DAY_MS + 9 * 3_600_000,
        Some(0.0),
        Some(cycle_end),
    );
    insert(
        &s,
        &p,
        today_start - DAY_MS + 18 * 3_600_000,
        Some(20.0),
        Some(cycle_end),
    );
    // Stray row from a different cycle, same day — must be filtered.
    insert(
        &s,
        &p,
        today_start - DAY_MS + 12 * 3_600_000,
        Some(99.0),
        Some(RESETS_B),
    );
    let actuals = compute_day_actuals(&s, &p, None, cycle_end, &Calendar(Local)).unwrap();
    let yest = actuals
        .iter()
        .find(|a| a.day_start_ms == today_start - DAY_MS)
        .unwrap();
    let v = yest.pct.unwrap();
    assert!((v - 20.0).abs() < 0.01, "expected 20%, got {}", v);
}

#[test]
fn day_actuals_match_a_reset_restamped_with_millisecond_jitter() {
    let p = unique_provider("actuals-jitter");
    let s = store();
    let now = now_at_hour(15);
    let today_start = local_start_of_day_ms(now);
    let cycle_end = today_start + DAY_MS;
    insert(
        &s,
        &p,
        today_start - DAY_MS + 9 * 3_600_000,
        Some(10.0),
        Some(cycle_end - 1_420),
    );
    insert(
        &s,
        &p,
        today_start - DAY_MS + 18 * 3_600_000,
        Some(20.0),
        Some(cycle_end + 1_490),
    );
    insert(
        &s,
        &p,
        today_start - 3_600_000,
        Some(20.0),
        Some(cycle_end + 15),
    );
    insert(&s, &p, now, Some(25.0), Some(cycle_end - 700));
    let actuals = compute_day_actuals(&s, &p, None, cycle_end, &Calendar(Local)).unwrap();
    let day = |start: i64| {
        actuals
            .iter()
            .find(|a| a.day_start_ms == start)
            .and_then(|a| a.pct)
    };
    assert!(
        (day(today_start - DAY_MS).unwrap() - 10.0).abs() < 0.01,
        "yesterday keeps its jittered samples"
    );
    assert!(
        (day(today_start).unwrap() - 5.0).abs() < 0.01,
        "today bridges across jittered resets"
    );
}

fn assert_riga_windows(calendar: Calendar<TransitionZone>, dates: [&str; 4], endpoints: [&str; 4]) {
    let starts = endpoints.map(utc);
    let windows = calendar.windows(starts[0], starts[3]).unwrap();
    assert_eq!(windows.len(), 3);
    for (i, window) in windows.iter().enumerate() {
        assert_eq!(window.date, date(dates[i]));
        assert_eq!((window.start_ms, window.end_ms), (starts[i], starts[i + 1]));
        assert_eq!(
            calendar.at(window.start_ms).unwrap().date_naive(),
            date(dates[i])
        );
        assert_eq!(
            calendar.at(window.end_ms - 1).unwrap().date_naive(),
            date(dates[i])
        );
        assert_eq!(
            calendar.at(window.end_ms).unwrap().date_naive(),
            date(dates[i + 1])
        );
    }
    for midnight in &starts[1..3] {
        for sample in [midnight - 1, *midnight, midnight + 1] {
            assert_eq!(
                windows
                    .iter()
                    .filter(|w| sample >= w.start_ms && sample < w.end_ms)
                    .count(),
                1
            );
        }
    }
    let actuals = compute_day_actuals(&store(), "calendar", None, starts[3], &calendar).unwrap();
    assert!(actuals
        .windows(2)
        .all(|pair| pair[0].day_start_ms < pair[1].day_start_ms));
    let keys: Vec<_> = actuals
        .iter()
        .map(|a| calendar.at(a.day_start_ms).unwrap().date_naive())
        .collect();
    assert!(keys
        .windows(2)
        .all(|pair| pair[0].succ_opt() == Some(pair[1])));
}

#[test]
fn riga_spring_has_contiguous_civil_dates_and_exact_utc_endpoints() {
    assert_riga_windows(
        riga_spring(),
        ["2026-03-28", "2026-03-29", "2026-03-30", "2026-03-31"],
        [
            "2026-03-27T22:00:00Z",
            "2026-03-28T22:00:00Z",
            "2026-03-29T21:00:00Z",
            "2026-03-30T21:00:00Z",
        ],
    );
}

#[test]
fn riga_fall_has_contiguous_civil_dates_and_exact_utc_endpoints() {
    assert_riga_windows(
        riga_fall(),
        ["2026-10-24", "2026-10-25", "2026-10-26", "2026-10-27"],
        [
            "2026-10-23T21:00:00Z",
            "2026-10-24T21:00:00Z",
            "2026-10-25T22:00:00Z",
            "2026-10-26T22:00:00Z",
        ],
    );
}

#[test]
fn calendar_crosses_month_and_year_with_unique_dates() {
    for (first, last, expected) in [
        (
            "2026-12-31T22:00:00Z",
            "2027-01-02T22:00:00Z",
            ["2027-01-01", "2027-01-02"],
        ),
        (
            "2026-01-30T22:00:00Z",
            "2026-02-01T22:00:00Z",
            ["2026-01-31", "2026-02-01"],
        ),
    ] {
        let windows = Calendar(FixedOffset::east_opt(7200).unwrap())
            .windows(utc(first), utc(last))
            .unwrap();
        assert_eq!(
            windows.iter().map(|w| w.date).collect::<Vec<_>>(),
            expected.map(date)
        );
        assert_eq!(windows[0].end_ms, windows[1].start_ms);
        assert!(windows.iter().all(|w| w.end_ms - w.start_ms == DAY_MS));
    }
}

#[test]
fn actuals_attribute_midnight_and_both_repeated_hours_once() {
    let calendar = riga_fall();
    let s = store();
    let p = unique_provider("fall-membership");
    let reset = utc("2026-10-26T10:00:00Z");
    let points = [
        ("2026-10-24T20:00:00Z", 0.0),
        ("2026-10-24T20:59:59.999Z", 1.0),
        ("2026-10-24T21:00:00Z", 3.0),
        ("2026-10-24T21:00:00.001Z", 6.0),
        ("2026-10-25T00:30:00Z", 10.0),
        ("2026-10-25T01:30:00Z", 15.0),
        ("2026-10-25T21:59:59.999Z", 21.0),
        ("2026-10-25T22:00:00Z", 28.0),
        ("2026-10-25T22:00:00.001Z", 36.0),
        ("2026-10-26T09:59:59.999Z", 45.0),
        ("2026-10-26T10:00:00Z", 99.0),
        ("2026-10-26T10:00:00.001Z", 100.0),
    ];
    for (at, pct) in points {
        insert(&s, &p, utc(at), Some(pct), Some(reset));
    }
    assert_eq!(calendar.at(utc("2026-10-25T00:30:00Z")).unwrap().hour(), 3);
    assert_eq!(calendar.at(utc("2026-10-25T01:30:00Z")).unwrap().hour(), 3);
    let actuals = compute_day_actuals(&s, &p, None, reset, &calendar).unwrap();
    let totals: Vec<_> = actuals.iter().filter_map(|a| a.pct).collect();
    assert_eq!(totals, [1.0, 20.0, 24.0]);
    assert_eq!(totals.iter().sum::<f64>(), 45.0);
    assert_eq!(
        latest_weekly(&s, &p, None, reset - 1).unwrap(),
        Some((45.0, reset))
    );
    let windows = calendar.windows(reset - 7 * DAY_MS, reset).unwrap();
    assert_eq!(windows.last().unwrap().end_ms, reset);
    assert!(windows.first().unwrap().start_ms <= reset - 7 * DAY_MS);
    assert!(windows.first().unwrap().end_ms > reset - 7 * DAY_MS);
}

#[test]
fn spring_actuals_share_midnight_endpoints_without_drifting_an_hour() {
    let calendar = riga_spring();
    let s = store();
    let p = unique_provider("spring-membership");
    let reset = utc("2026-03-30T21:00:00Z");
    for (at, pct) in [
        ("2026-03-28T21:00:00Z", 0.0),
        ("2026-03-28T21:59:59.999Z", 1.0),
        ("2026-03-28T22:00:00Z", 3.0),
        ("2026-03-28T22:00:00.001Z", 6.0),
        ("2026-03-29T00:59:59.999Z", 10.0),
        ("2026-03-29T01:00:00Z", 15.0),
        ("2026-03-29T20:59:59.999Z", 21.0),
        ("2026-03-29T21:00:00Z", 28.0),
        ("2026-03-29T21:00:00.001Z", 36.0),
        ("2026-03-30T21:00:00Z", 99.0),
    ] {
        insert(&s, &p, utc(at), Some(pct), Some(reset));
    }
    let actuals = compute_day_actuals(&s, &p, None, reset, &calendar).unwrap();
    let totals: Vec<_> = actuals.iter().filter_map(|a| a.pct).collect();
    assert_eq!(totals, [1.0, 20.0, 15.0]);
    assert_eq!(totals.iter().sum::<f64>(), 36.0);
}

#[test]
fn future_weekday_sums_visit_each_civil_date_once() {
    let estimates = [1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0, 1000000.0];
    for (calendar, now, end) in [
        (
            riga_spring(),
            "2026-03-28T10:00:00Z",
            "2026-03-30T21:00:00Z",
        ),
        (
            riga_spring(),
            "2026-03-28T10:00:00Z",
            "2026-03-29T21:30:00Z",
        ),
        (riga_fall(), "2026-10-24T10:00:00Z", "2026-10-26T22:00:00Z"),
    ] {
        assert_eq!(
            calendar
                .future_spend(utc(now), utc(end), &estimates)
                .unwrap(),
            11.0
        );
        assert_eq!(
            calendar
                .future_spend(
                    utc(now),
                    calendar.shift_start(utc(now), 1).unwrap(),
                    &estimates
                )
                .unwrap(),
            0.0
        );
    }
}

#[test]
fn baseline_lookback_and_day_grouping_share_the_injected_calendar() {
    for (calendar, now, boundary, previous, early, late, repeated, end) in [
        (
            riga_spring(),
            "2026-03-30T10:00:00Z",
            "2025-12-29T22:00:00Z",
            "2025-12-29T21:30:00Z",
            "2026-03-28T22:30:00Z",
            "2026-03-29T20:30:00Z",
            "2026-03-29T00:30:00Z",
            "2026-03-29T21:00:00Z",
        ),
        (
            riga_fall(),
            "2026-10-26T10:00:00Z",
            "2026-07-27T21:00:00Z",
            "2026-07-27T20:30:00Z",
            "2026-10-24T21:30:00Z",
            "2026-10-25T21:30:00Z",
            "2026-10-25T01:30:00Z",
            "2026-10-25T22:00:00Z",
        ),
    ] {
        assert_eq!(calendar.shift_start(utc(now), -90).unwrap(), utc(boundary));
        let s = store();
        let p = unique_provider("calendar-baseline");
        for (at, pct) in [
            (utc(previous) - 1000, 0.0),
            (utc(previous), 99.0),
            (utc(boundary), 0.0),
            (utc(boundary) + 1000, 2.0),
            (utc(early), 10.0),
            (utc(repeated), 13.0),
            (utc(late), 20.0),
            (utc(end), 70.0),
            (utc(end) + 1000, 99.0),
        ] {
            insert(&s, &p, at, Some(pct), Some(RESETS_A));
        }
        let b = compute_baseline_with_calendar(&s, &p, None, utc(now), &calendar).unwrap();
        assert_eq!(b.dow_n[0], 1);
        assert_eq!(b.dow_total[0], 10.0);
        assert_eq!(b.dow_n.iter().sum::<u32>(), 2);
        assert_eq!(b.wd_total_avg, 2.0);
    }
}

#[test]
fn midnight_resolution_chooses_earliest_fold_and_first_valid_gap_instant() {
    let day = date("2026-01-02");
    let gap = Calendar(TransitionZone {
        at_ms: utc("2026-01-02T00:00:00Z"),
        before: 0,
        after: 3600,
    });
    assert_eq!(gap.start(day).unwrap(), utc("2026-01-02T00:00:00Z"));
    assert_eq!(gap.at(gap.start(day).unwrap()).unwrap().hour(), 1);
    let fold = Calendar(TransitionZone {
        at_ms: utc("2026-01-02T00:00:00Z"),
        before: 3600,
        after: 0,
    });
    assert_eq!(fold.start(day).unwrap(), utc("2026-01-01T23:00:00Z"));
    let skipped = Calendar(TransitionZone {
        at_ms: utc("2026-01-02T12:00:00Z"),
        before: -43200,
        after: 43200,
    });
    assert!(skipped
        .start(day)
        .unwrap_err()
        .contains("local civil date does not exist"));
}
