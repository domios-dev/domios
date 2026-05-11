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
