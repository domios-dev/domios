use super::*;
use crate::usage::store::UsageStore;

const RESETS_A: i64 = 9_000_000_000_000;
const RESETS_B: i64 = 9_000_000_000_001;

// Unique provider per test to avoid the static BASELINE_CACHE bleeding between
// parallel test threads.
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
    let base = local_start_of_day_ms(chrono::Utc::now().timestamp_millis());
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
    let today_start = local_start_of_day_ms(chrono::Utc::now().timestamp_millis());
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
    let today_start = local_start_of_day_ms(chrono::Utc::now().timestamp_millis());
    for d in 1..=14 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        let delta = if dow == 0 || dow == 6 { 200.0 } else { 1.0 };
        insert(&s, &p, day + 12 * 3_600_000, Some(0.0), Some(RESETS_A));
        insert(&s, &p, day + 14 * 3_600_000, Some(delta), Some(RESETS_A));
    }
    let b = compute_baseline(&s, &p, None, today_start + 12 * 3_600_000).unwrap();
    assert!(b.we_to_wd_ratio <= 2.0 + 1e-9, "ratio = {}", b.we_to_wd_ratio);
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
    assert_eq!(b_cached_dow_n_sum, b1_dow_n_sum, "cached should not see new rows");
    invalidate_cache(&p, None);
    let b2 = baseline_for(&s, &p, None, now).unwrap();
    let b2_dow_n_sum: u32 = b2.dow_n.iter().sum();
    assert!(b2_dow_n_sum > b1_dow_n_sum, "fresh baseline should see new rows");
}

#[test]
fn shape_skips_offdevice_jump_pair() {
    // Setup: 5 normal pairs during the morning (1h apart, 4% each), then
    // ONE giant pair after a 6h gap with 35% delta (off-device work).
    // The shape must NOT pin the 35% to a single hour — that pair gets
    // dropped from per-hour aggregation entirely.
    let p = unique_provider("offdev");
    let s = store();
    let today_start = local_start_of_day_ms(chrono::Utc::now().timestamp_millis());
    // Need ≥3 days for confidence>=Medium; replicate across 4 weekdays.
    let mut wd_days_seeded = 0;
    for d in 1..=14 {
        let day = today_start - d * DAY_MS;
        let dow = local_from_ms(day).weekday().num_days_from_sunday();
        if dow == 0 || dow == 6 {
            continue;
        }
        // 9:00 — 13:00, 4 pairs of +4%
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
        h19_share, h10_share,
    );
    // Shape still sums to 1.
    let sum: f64 = b.wd_hour_share.iter().sum();
    assert!((sum - 1.0).abs() < 0.01, "shape sum {}", sum);
}

#[test]
fn median_resists_vacation_outlier() {
    // 10 weeks at ~28% Monday + 1 vacation week at 4% Monday.
    // Mean would be ~26%, median should be ~28% — vacation doesn't drag.
    // Realistic sampling: 8 samples through the day (~4% per pair).
    let p = unique_provider("vac");
    let s = store();
    let today_start = local_start_of_day_ms(chrono::Utc::now().timestamp_millis());
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
    // 23:00 yesterday = 1 hour before midnight → within 2h window
    let near_midnight = today_start_ms - 60 * 60 * 1000;
    insert(&s, &p, near_midnight, Some(30.0), Some(RESETS_A));
    insert(&s, &p, now, Some(40.0), Some(RESETS_A));
    let delta = today_delta(&s, &p, None, now, 40.0, RESETS_A).unwrap();
    // 40 - 30 = 10; pre-midnight reading was close enough, use cross-day bridge
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
    // 14:00 yesterday = 10 hours before midnight → gap exceeds 2h window
    let far_before_midnight = today_start_ms - 10 * 60 * 60 * 1000;
    insert(&s, &p, far_before_midnight, Some(34.0), Some(RESETS_A));
    // Only one reading today → today_min = current_pct → from_today_min = 0
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
    let today_start = local_start_of_day_ms(chrono::Utc::now().timestamp_millis());
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
    // Two readings today: 50% at 10h, 58% at 14h
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
    let actuals = compute_day_actuals(&s, &p, None, now, cycle_end).unwrap();
    // 7 days back from cycle_end, aligned to local-start-of-day → at least 7,
    // possibly 8 if the cycle's start time crossed a midnight boundary.
    assert!((7..=8).contains(&actuals.len()), "got {} buckets", actuals.len());
    assert!(actuals.iter().all(|a| a.pct.is_none()), "empty db → all None");
}

#[test]
fn day_actuals_sparse_history_attributes_only_observed_spend() {
    // Cycle started 7 days ago. Only days 3–today have polling data.
    // The first two empty days must stay None (not inherit cumulative).
    let p = unique_provider("actuals-sparse");
    let s = store();
    let now = now_at_hour(15);
    let today_start = local_start_of_day_ms(now);
    let cycle_end = today_start + DAY_MS;
    // Day -4: 5 → 10
    insert(&s, &p, today_start - 4 * DAY_MS + 9 * 3_600_000, Some(5.0), Some(cycle_end));
    insert(&s, &p, today_start - 4 * DAY_MS + 15 * 3_600_000, Some(10.0), Some(cycle_end));
    // Day -3: 12 → 14
    insert(&s, &p, today_start - 3 * DAY_MS + 10 * 3_600_000, Some(12.0), Some(cycle_end));
    insert(&s, &p, today_start - 3 * DAY_MS + 18 * 3_600_000, Some(14.0), Some(cycle_end));
    // Today: 14 (last yesterday EOD-ish) → 22 now. yest 22:00 = 14.
    insert(&s, &p, today_start - 2 * 3_600_000, Some(14.0), Some(cycle_end));
    insert(&s, &p, now, Some(22.0), Some(cycle_end));
    let actuals = compute_day_actuals(&s, &p, None, today_start, cycle_end).unwrap();
    // Today bucket is the LAST one (day_start_ms == today_start).
    let today_bucket = actuals.iter().find(|a| a.day_start_ms == today_start).unwrap();
    // Bridge 14→14 = 0 + within-day no pairs (just one sample at now) → 0?
    // Actually two samples: yest-22:00 (gets filtered by day_start cutoff) and now.
    // So today has 1 sample. Bridge 14→22 (within 2h gap) = +8. Total 8.
    assert!(today_bucket.pct.is_some());
    let v = today_bucket.pct.unwrap();
    assert!((v - 8.0).abs() < 0.01, "today bucket got {}", v);
    // Days with no samples should be None, not 0.
    let empty_days = actuals.iter().filter(|a| a.pct.is_none()).count();
    assert!(empty_days >= 2, "expected ≥2 empty days, got {}", empty_days);
}

#[test]
fn day_actuals_skip_other_cycle_rows() {
    // A snapshot from a different cycle (RESETS_B) appearing within this
    // cycle's day window must not contribute to that day's delta.
    let p = unique_provider("actuals-cycle-filter");
    let s = store();
    let now = now_at_hour(15);
    let today_start = local_start_of_day_ms(now);
    let cycle_end = today_start + DAY_MS;
    insert(&s, &p, today_start - DAY_MS + 9 * 3_600_000, Some(0.0), Some(cycle_end));
    insert(&s, &p, today_start - DAY_MS + 18 * 3_600_000, Some(20.0), Some(cycle_end));
    // Stray row from a different cycle, same day — must be filtered.
    insert(&s, &p, today_start - DAY_MS + 12 * 3_600_000, Some(99.0), Some(RESETS_B));
    let actuals = compute_day_actuals(&s, &p, None, today_start, cycle_end).unwrap();
    let yest = actuals.iter().find(|a| a.day_start_ms == today_start - DAY_MS).unwrap();
    let v = yest.pct.unwrap();
    assert!((v - 20.0).abs() < 0.01, "expected 20%, got {}", v);
}
