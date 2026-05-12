//! Projection of usage to end-of-today and end-of-cycle.
//!
//! Logic ported from `references/usage-projection/usage_projection_sim.py`
//! (validated against synthetic profiles + a SQL integration test there).
//!
//! Strategy in one paragraph:
//! - Look at the last 14 days of `usage_snapshots` (excluding today).
//! - For each day, bucket samples by weekday/weekend and by hour-of-day,
//!   computing per-hour delta = `MAX(weekly_pct) - MIN(weekly_pct)`.
//! - Average those hour buckets to build an "expected %/hour" curve per
//!   weekday and per weekend.
//! - "Rest of today" = sum of expected hourly rates from the current hour
//!   through midnight, with the current hour's slice blended 60/40 against
//!   the user's recent rate (last 30 min) when there's recent activity.
//! - "End of cycle" = end-of-today + Σ (per-future-day baseline).
//! - When we have less than 3 historical days of data we still emit a
//!   projection but flag confidence = "low" and fall back to a hardcoded
//!   office-hours hourly profile so the markers don't disappear.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::store::UsageStoreHandle;

const DAY_MS: i64 = 86_400_000;
const RECENT_WINDOW_MS: i64 = 30 * 60 * 1000;
const BASELINE_LOOKBACK_DAYS: i64 = 90;
/// Sample-pair gap threshold. Pairs further apart than this likely span a
/// period the poller wasn't running (laptop closed, working on another
/// device) — the cumulative jump can't be cleanly attributed to a single
/// hour, so we skip the pair from per-hour shape aggregation.
const SHAPE_PAIR_GAP_MS: i64 = 4 * 60 * 60 * 1000;
/// Pairs whose delta exceeds this likely represent off-device work that
/// happened in batches. Don't blame one hour for it.
const SHAPE_PAIR_DELTA_MAX: f64 = 25.0;
/// How long a baseline is kept in the in-memory cache. New rows from today
/// don't shift the rolling baseline (we exclude today), so refreshing once
/// per local-day is enough; we also re-check after this many seconds to
/// avoid serving stale data across DST shifts / clock changes.
const BASELINE_CACHE_TTL_S: i64 = 6 * 60 * 60;

// Default daily profile = office hours 9:00–20:00, peak mid-morning + mid-afternoon,
// taper to 0 by 20:00. Shape is normalized later; relative values matter, not absolute.
// At 19:00 the cumulative share is ~0.95, so projection only adds a sliver for the
// remaining hour instead of stretching activity through midnight.
const DEFAULT_HOURLY_WEEKDAY: [f64; 24] = [
    // 0  1   2   3   4   5   6
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    // 7  8    9    10   11   12   13
    0.0, 0.6, 1.3, 1.5, 1.5, 1.0, 0.8,
    // 14   15   16   17   18  19
    1.3, 1.4, 1.3, 1.0, 0.7, 0.3,
    // 20 21 22 23
    0.0, 0.0, 0.0, 0.0,
];
// Weekend: shorter window, peak around lunch, done by 18.
const DEFAULT_HOURLY_WEEKEND: [f64; 24] = [
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    0.0, 0.3, 0.6, 0.8, 0.8, 0.6, 0.4, 0.3, 0.2, 0.1,
    0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyProjection {
    pub end_of_today_pct: f64,
    pub end_of_cycle_pct: f64,
    pub avg_weekday_spend: f64,
    pub avg_weekend_spend: f64,
    /// Per day-of-week projected spend (0=Sun..6=Sat). Lets the UI paint each
    /// future day's bar with its own scale instead of one weekday average.
    pub dow_estimates: [f64; 7],
    pub rate_per_min_recent: f64,
    pub rest_of_today_est: f64,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionProjection {
    pub projected_pct: f64,
    pub rate_per_min: f64,
    pub will_exceed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionResponse {
    pub session: Option<SessionProjection>,
    pub weekly: Option<WeeklyProjection>,
}

#[derive(Debug, Clone)]
struct Baseline {
    /// Normalized share of daily spend per hour (sums to 1.0). Plan-independent
    /// — captures rhythm of work, not absolute %.
    wd_hour_share: [f64; 24],
    we_hour_share: [f64; 24],
    /// Historical average daily total in absolute %. Used as a fallback prior
    /// when today's data is too sparse to extrapolate.
    wd_total_avg: f64,
    we_total_avg: f64,
    /// Per day-of-week historical average total (0=Sun..6=Sat). Captures
    /// Monday-heavy / Friday-light patterns the bucket average flattens out.
    dow_total: [f64; 7],
    /// Sample-day count per DOW — if 0, fall back to wd/we average.
    dow_n: [u32; 7],
    /// weekend_total / weekday_total — how much quieter weekends are.
    /// Plan-independent (cancels out the tier limit).
    we_to_wd_ratio: f64,
    confidence: Confidence,
    computed_day: i32,
    computed_at_ms: i64,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct CacheKey {
    provider: String,
    account_id: Option<String>,
}

static BASELINE_CACHE: OnceLock<Mutex<HashMap<CacheKey, Baseline>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<CacheKey, Baseline>> {
    BASELINE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn local_yyyymmdd(ms: i64) -> i32 {
    let dt = local_from_ms(ms);
    dt.year() * 10_000 + (dt.month() as i32) * 100 + (dt.day() as i32)
}

/// Force-drop any cached baseline. Called after a successful probe insert so
/// the next projection request picks up fresh data — cheap, and avoids
/// staleness without polling.
pub fn invalidate_cache(provider: &str, account_id: Option<&str>) {
    let key = CacheKey {
        provider: provider.to_string(),
        account_id: account_id.map(str::to_string),
    };
    if let Ok(mut guard) = cache().lock() {
        guard.remove(&key);
    }
}

pub fn compute(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<ProjectionResponse, String> {
    let session = compute_session(store, provider, account_id, now_ms)?;
    let weekly = compute_weekly(store, provider, account_id, now_ms)?;
    Ok(ProjectionResponse { session, weekly })
}

fn compute_session(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<Option<SessionProjection>, String> {
    store.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                r#"SELECT captured_at, session_pct, session_resets_at
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at <= ?3
                       AND session_pct IS NOT NULL
                       AND session_resets_at IS NOT NULL
                     ORDER BY captured_at DESC
                     LIMIT 1"#,
            )
            .map_err(|e| format!("prepare latest: {}", e))?;
        let latest: Option<(i64, f64, i64)> = stmt
            .query_row(params![provider, account_id, now_ms], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, i64>(2)?))
            })
            .ok();
        let (_, current_pct, resets_at) = match latest {
            Some(t) => t,
            None => return Ok(None),
        };

        let mut window_stmt = conn
            .prepare(
                r#"SELECT captured_at, session_pct
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at <= ?3
                       AND session_resets_at = ?4
                       AND session_pct IS NOT NULL
                     ORDER BY captured_at ASC"#,
            )
            .map_err(|e| format!("prepare session window: {}", e))?;
        let rows: Vec<(i64, f64)> = window_stmt
            .query_map(params![provider, account_id, now_ms, resets_at], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
            })
            .map_err(|e| format!("query session window: {}", e))?
            .filter_map(Result::ok)
            .collect();
        if rows.len() < 2 {
            return Ok(None);
        }
        let first = rows.first().copied().unwrap();
        let last = rows.last().copied().unwrap();
        let dt_ms = last.0 - first.0;
        if dt_ms < 60_000 {
            return Ok(None);
        }
        let d_pct = (last.1 - first.1).max(0.0);
        let rate_per_ms = d_pct / dt_ms as f64;
        if rate_per_ms <= 0.0 {
            return Ok(None);
        }
        let remaining_ms = (resets_at - now_ms).max(0) as f64;
        let projected = (current_pct + rate_per_ms * remaining_ms).min(200.0);
        Ok(Some(SessionProjection {
            projected_pct: projected,
            rate_per_min: rate_per_ms * 60_000.0,
            will_exceed: projected >= 100.0,
        }))
    })
}

fn compute_weekly(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<Option<WeeklyProjection>, String> {
    let (current_pct, cycle_end_ms) = match latest_weekly(store, provider, account_id, now_ms)? {
        Some(t) => t,
        None => return Ok(None),
    };
    let baseline = baseline_for(store, provider, account_id, now_ms)?;
    let rate_per_min = recent_rate(store, provider, account_id, now_ms)?;
    let today_so_far = today_delta(store, provider, account_id, now_ms, current_pct, cycle_end_ms)?;

    let now_local = local_from_ms(now_ms);
    let today_dow = now_local.weekday().num_days_from_sunday();
    let today_is_we = today_dow == 0 || today_dow == 6;
    let shape: [f64; 24] = if today_is_we {
        baseline.we_hour_share
    } else {
        baseline.wd_hour_share
    };
    // Prefer per-DOW total when we have enough samples for today's DOW, else
    // fall back to the weekend/weekday bucket average.
    let hist_today_total = if baseline.dow_n[today_dow as usize] >= 3 {
        baseline.dow_total[today_dow as usize]
    } else if today_is_we {
        baseline.we_total_avg
    } else {
        baseline.wd_total_avg
    };

    // How much of today's daily shape has already elapsed.
    let cur_hour = now_local.hour() as usize;
    let minute_frac = (now_local.minute() as f64 / 60.0).clamp(0.0, 1.0);
    let mut share_used: f64 = (0..cur_hour).map(|h| shape[h]).sum();
    share_used += shape[cur_hour] * minute_frac;
    share_used = share_used.clamp(0.0, 1.0);

    // Estimate today's full-day total by combining observed pace with the
    // historical prior. Trust the prior more early in the day, less late
    // (when the day's shape has mostly played out and extrapolation is solid).
    // Then floor at today_so_far so we never project less than what's already
    // happened — if the user is burning faster than the prior, re-anchor.
    let est_today_total: f64 = if share_used >= 0.05 && today_so_far > 0.0 {
        let extrapolated = today_so_far / share_used;
        let hist_weight = (1.0 - share_used).clamp(0.2, 0.8);
        let extr_weight = 1.0 - hist_weight;
        let blended = if hist_today_total > 0.0 {
            extr_weight * extrapolated + hist_weight * hist_today_total
        } else {
            extrapolated
        };
        blended.max(today_so_far)
    } else {
        hist_today_total.max(today_so_far)
    };

    let rest_of_today = (est_today_total - today_so_far).max(0.0);
    let end_of_today = current_pct + rest_of_today;

    // Per-DOW estimate for each future day: blend today's pace-relative scale
    // with the historical DOW-specific average. The "scale factor" is how today's
    // observed total compares to what's typical for today's DOW — if today is
    // running 1.2× usual Monday, project 1.2× usual Tuesday too.
    let today_dow_hist = if baseline.dow_n[today_dow as usize] >= 3 {
        baseline.dow_total[today_dow as usize]
    } else if today_is_we {
        baseline.we_total_avg
    } else {
        baseline.wd_total_avg
    };
    let scale = if today_dow_hist > 0.0 {
        (est_today_total / today_dow_hist).clamp(0.3, 3.0)
    } else {
        1.0
    };

    let dow_estimate = |dow: usize| -> f64 {
        let hist = if baseline.dow_n[dow] >= 3 {
            baseline.dow_total[dow]
        } else if dow == 0 || dow == 6 {
            baseline.we_total_avg
        } else {
            baseline.wd_total_avg
        };
        // 50% scaled-by-today-pace + 50% pure historical = soft revert to mean.
        0.5 * (hist * scale) + 0.5 * hist
    };

    let mut end_of_cycle = end_of_today;
    let today_start_ms = local_start_of_day_ms(now_ms);
    let mut cursor = today_start_ms + DAY_MS;
    while cursor < cycle_end_ms {
        let dow = local_from_ms(cursor).weekday().num_days_from_sunday() as usize;
        end_of_cycle += dow_estimate(dow);
        cursor += DAY_MS;
    }

    // For the response API: report the bucket-averaged weekday/weekend numbers
    // (legacy compatibility) AND expose per-DOW so the frontend bar chart can
    // paint each future day with its own height.
    let est_wd_total = (1..=5).map(|d| dow_estimate(d)).sum::<f64>() / 5.0;
    let est_we_total = (dow_estimate(0) + dow_estimate(6)) / 2.0;
    let dow_estimates: [f64; 7] = std::array::from_fn(|d| dow_estimate(d));

    Ok(Some(WeeklyProjection {
        end_of_today_pct: end_of_today.min(200.0),
        end_of_cycle_pct: end_of_cycle.min(200.0),
        avg_weekday_spend: est_wd_total,
        avg_weekend_spend: est_we_total,
        dow_estimates,
        rate_per_min_recent: rate_per_min,
        rest_of_today_est: rest_of_today,
        confidence: baseline.confidence.clone(),
    }))
}

fn today_delta(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
    current_pct: f64,
    current_resets_at: i64,
) -> Result<f64, String> {
    let today_start_ms = local_start_of_day_ms(now_ms);
    store.with_conn(|conn| {
        let today_min: Option<f64> = conn
            .prepare(
                r#"SELECT MIN(weekly_pct)
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at >= ?3
                       AND captured_at <= ?4
                       AND weekly_pct IS NOT NULL"#,
            )
            .map_err(|e| format!("prepare today_min: {}", e))?
            .query_row(
                params![provider, account_id, today_start_ms, now_ms],
                |r| r.get::<_, Option<f64>>(0),
            )
            .ok()
            .flatten();

        // Fetch captured_at too so we can detect a large gap before midnight.
        let yest: Option<(f64, i64, i64)> = conn
            .prepare(
                r#"SELECT weekly_pct, weekly_resets_at, captured_at
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at < ?3
                       AND weekly_pct IS NOT NULL
                       AND weekly_resets_at IS NOT NULL
                     ORDER BY captured_at DESC
                     LIMIT 1"#,
            )
            .map_err(|e| format!("prepare yest: {}", e))?
            .query_row(params![provider, account_id, today_start_ms], |r| {
                Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })
            .ok();

        // Cross-midnight bridge: only usable when the pre-midnight reading was
        // taken within 2 hours of midnight. A bigger gap means we don't know
        // when the spending happened — crediting the whole delta to today would
        // inflate today_so_far and break the projection.
        const MAX_MIDNIGHT_GAP_MS: i64 = 2 * 60 * 60 * 1000;
        let from_yest = yest.and_then(|(prev_pct, prev_resets, prev_at)| {
            let gap = today_start_ms - prev_at;
            if prev_resets == current_resets_at
                && prev_pct <= current_pct
                && gap <= MAX_MIDNIGHT_GAP_MS
            {
                Some(current_pct - prev_pct)
            } else {
                None
            }
        });
        let from_today_min = today_min
            .map(|m| (current_pct - m).max(0.0))
            .unwrap_or(0.0);
        Ok(from_yest.unwrap_or(from_today_min))
    })
}

fn latest_weekly(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<Option<(f64, i64)>, String> {
    store.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                r#"SELECT weekly_pct, weekly_resets_at
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at <= ?3
                       AND weekly_pct IS NOT NULL
                       AND weekly_resets_at IS NOT NULL
                     ORDER BY captured_at DESC
                     LIMIT 1"#,
            )
            .map_err(|e| format!("prepare latest weekly: {}", e))?;
        let row: Option<(f64, i64)> = stmt
            .query_row(params![provider, account_id, now_ms], |r| {
                Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?))
            })
            .ok();
        Ok(row)
    })
}

fn recent_rate(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<f64, String> {
    store.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                r#"SELECT captured_at, weekly_pct
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at >= ?3
                       AND captured_at <= ?4
                       AND weekly_pct IS NOT NULL
                     ORDER BY captured_at ASC"#,
            )
            .map_err(|e| format!("prepare recent: {}", e))?;
        let recent: Vec<(i64, f64)> = stmt
            .query_map(
                params![provider, account_id, now_ms - RECENT_WINDOW_MS, now_ms],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)),
            )
            .map_err(|e| format!("query recent: {}", e))?
            .filter_map(Result::ok)
            .collect();
        if recent.len() < 2 {
            return Ok(0.0);
        }
        let first = recent.first().copied().unwrap();
        let last = recent.last().copied().unwrap();
        let dt_min = (last.0 - first.0) as f64 / 60_000.0;
        if dt_min <= 0.0 {
            return Ok(0.0);
        }
        Ok(((last.1 - first.1) / dt_min).max(0.0))
    })
}

fn baseline_for(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<Baseline, String> {
    let key = CacheKey {
        provider: provider.to_string(),
        account_id: account_id.map(str::to_string),
    };
    let today_yyyymmdd = local_yyyymmdd(now_ms);
    if let Ok(guard) = cache().lock() {
        if let Some(cached) = guard.get(&key) {
            let age_s = (now_ms - cached.computed_at_ms) / 1000;
            if cached.computed_day == today_yyyymmdd && age_s < BASELINE_CACHE_TTL_S {
                return Ok(cached.clone());
            }
        }
    }
    let fresh = compute_baseline(store, provider, account_id, now_ms)?;
    if let Ok(mut guard) = cache().lock() {
        guard.insert(key, fresh.clone());
    }
    Ok(fresh)
}

fn compute_baseline(
    store: &UsageStoreHandle,
    provider: &str,
    account_id: Option<&str>,
    now_ms: i64,
) -> Result<Baseline, String> {
    let today_start_ms = local_start_of_day_ms(now_ms);
    let lookback_start_ms = today_start_ms - BASELINE_LOOKBACK_DAYS * DAY_MS;

    store.with_conn(|conn| {
        let mut stmt = conn
            .prepare(
                r#"SELECT
                       strftime('%Y%m%d', captured_at / 1000, 'unixepoch', 'localtime') AS day,
                       CAST(strftime('%w', captured_at / 1000, 'unixepoch', 'localtime') AS INTEGER) AS dow,
                       captured_at,
                       weekly_pct,
                       weekly_resets_at
                     FROM usage_snapshots
                     WHERE provider = ?1
                       AND parsed_ok = 1
                       AND (?2 IS NULL OR account_id = ?2)
                       AND captured_at >= ?3
                       AND captured_at < ?4
                       AND weekly_pct IS NOT NULL
                     ORDER BY captured_at ASC"#,
            )
            .map_err(|e| format!("prepare samples: {}", e))?;
        let samples: Vec<(String, i64, i64, f64, Option<i64>)> = stmt
            .query_map(
                params![provider, account_id, lookback_start_ms, today_start_ms],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, f64>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .map_err(|e| format!("query samples: {}", e))?
            .filter_map(Result::ok)
            .collect();

        // Bucket samples per local day. Each day records hour-of-sample increments
        // (consecutive sample deltas attributed to the later sample's hour). This
        // works on sparse histories — even one sample per hour produces signal.
        let mut day_buckets: HashMap<String, (bool, [f64; 24], Vec<(i64, f64, Option<i64>)>)> =
            HashMap::new();
        for (day, dow, captured_at, pct, resets_at) in samples {
            let is_we = dow == 0 || dow == 6;
            let entry = day_buckets
                .entry(day)
                .or_insert((is_we, [0.0; 24], Vec::new()));
            entry.0 = is_we;
            entry.2.push((captured_at, pct, resets_at));
        }

        // Per-day: track day_total (no shape-filter — captures off-device
        // jumps so vacation weeks / multi-device usage still feed median)
        // and hours[] (filtered — clean shape signal). These diverge by
        // design: total tells us "how much did this day cost in absolute %",
        // shape tells us "how is that cost normally distributed across hours
        // when we observe it live".
        let mut day_hours: HashMap<String, (bool, [f64; 24], f64)> = HashMap::new();
        let mut day_dows: HashMap<String, usize> = HashMap::new();
        for (day, (is_we, _empty_hours, mut samples)) in day_buckets {
            samples.sort_by_key(|s| s.0);
            if let Some(first) = samples.first() {
                let dow = local_from_ms(first.0).weekday().num_days_from_sunday() as usize;
                day_dows.insert(day.clone(), dow);
            }
            let mut hours = [0.0_f64; 24];
            let mut day_total = 0.0_f64;
            for w in samples.windows(2) {
                let (ts_a, pct_a, reset_a) = w[0];
                let (ts_b, pct_b, reset_b) = w[1];
                let mut delta = pct_b - pct_a;
                if let (Some(a), Some(b)) = (reset_a, reset_b) {
                    if a != b {
                        continue; // cycle reset boundary — drop pair entirely
                    }
                }
                if delta < 0.0 {
                    delta = 0.0;
                }
                // ALWAYS counts toward day total (capture genuine activity).
                day_total += delta;
                // SHAPE-only filter: attributing a giant off-device jump to
                // one hour distorts the per-hour curve. Cap how much one pair
                // can contribute to hours[] without losing the magnitude
                // from day_total.
                let gap_ms = ts_b - ts_a;
                if gap_ms > SHAPE_PAIR_GAP_MS || delta > SHAPE_PAIR_DELTA_MAX {
                    continue;
                }
                let h = local_from_ms(ts_b).hour().clamp(0, 23) as usize;
                hours[h] += delta;
            }
            day_hours.insert(day, (is_we, hours, day_total));
        }

        // Per day: normalize to shape (sum=1), then average across same dow-group.
        // For totals we collect the per-day list to take a MEDIAN later —
        // robust against vacation weeks / odd outliers that would bias a mean.
        let mut wd_share_sum = [0.0_f64; 24];
        let mut we_share_sum = [0.0_f64; 24];
        let mut wd_share_n = 0u32;
        let mut we_share_n = 0u32;
        let mut wd_totals: Vec<f64> = Vec::new();
        let mut we_totals: Vec<f64> = Vec::new();
        let mut dow_totals: [Vec<f64>; 7] = Default::default();
        let mut wd_n = 0u32;
        let mut we_n = 0u32;
        for (day, (is_we, hours, day_total)) in &day_hours {
            if *day_total <= 0.0 {
                continue;
            }
            let dow_idx: usize = day_dows.get(day).copied().unwrap_or(0);
            dow_totals[dow_idx].push(*day_total);
            if *is_we {
                we_totals.push(*day_total);
                we_n += 1;
            } else {
                wd_totals.push(*day_total);
                wd_n += 1;
            }
            // Shape contribution: only if this day had unfiltered pair signal.
            let hour_total: f64 = hours.iter().sum();
            if hour_total <= 0.0 {
                continue;
            }
            if *is_we {
                for h in 0..24 {
                    we_share_sum[h] += hours[h] / hour_total;
                }
                we_share_n += 1;
            } else {
                for h in 0..24 {
                    wd_share_sum[h] += hours[h] / hour_total;
                }
                wd_share_n += 1;
            }
        }

        fn median(mut xs: Vec<f64>) -> f64 {
            if xs.is_empty() {
                return 0.0;
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = xs.len();
            if n % 2 == 1 {
                xs[n / 2]
            } else {
                (xs[n / 2 - 1] + xs[n / 2]) / 2.0
            }
        }
        let wd_total_sum = median(wd_totals);
        let we_total_sum = median(we_totals);

        let total_days = wd_n + we_n;
        let confidence = match total_days {
            0..=2 => Confidence::Low,
            3..=6 => Confidence::Medium,
            _ => Confidence::High,
        };

        let default_wd_sum: f64 = DEFAULT_HOURLY_WEEKDAY.iter().sum();
        let default_we_sum: f64 = DEFAULT_HOURLY_WEEKEND.iter().sum();
        let default_wd_share: [f64; 24] = std::array::from_fn(|h| {
            if default_wd_sum > 0.0 { DEFAULT_HOURLY_WEEKDAY[h] / default_wd_sum } else { 0.0 }
        });
        let default_we_share: [f64; 24] = std::array::from_fn(|h| {
            if default_we_sum > 0.0 { DEFAULT_HOURLY_WEEKEND[h] / default_we_sum } else { 0.0 }
        });

        let wd_hour_share: [f64; 24] = if wd_share_n >= 2 {
            std::array::from_fn(|h| wd_share_sum[h] / wd_share_n as f64)
        } else {
            default_wd_share
        };
        let we_hour_share: [f64; 24] = if we_share_n >= 2 {
            std::array::from_fn(|h| we_share_sum[h] / we_share_n as f64)
        } else {
            default_we_share
        };
        // wd_total_sum / we_total_sum are MEDIANS (renamed signature for clarity).
        let wd_total_avg = if wd_n > 0 { wd_total_sum } else { default_wd_sum };
        let we_total_avg = if we_n > 0 { we_total_sum } else { default_we_sum };
        let we_to_wd_ratio = if wd_total_avg > 0.0 {
            (we_total_avg / wd_total_avg).clamp(0.02, 2.0)
        } else {
            0.2
        };

        let mut dow_total = [0.0_f64; 7];
        let mut dow_n_out = [0u32; 7];
        for d in 0..7 {
            let n = dow_totals[d].len() as u32;
            dow_n_out[d] = n;
            dow_total[d] = if n > 0 {
                median(std::mem::take(&mut dow_totals[d]))
            } else {
                if d == 0 || d == 6 { we_total_avg } else { wd_total_avg }
            };
        }

        Ok(Baseline {
            wd_hour_share,
            we_hour_share,
            wd_total_avg,
            we_total_avg,
            dow_total,
            dow_n: dow_n_out,
            we_to_wd_ratio,
            confidence,
            computed_day: local_yyyymmdd(now_ms),
            computed_at_ms: now_ms,
        })
    })
}

fn local_from_ms(ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(|| Local.timestamp_opt(0, 0).unwrap())
}

fn local_start_of_day_ms(ms: i64) -> i64 {
    let dt = local_from_ms(ms);
    let start = Local
        .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
        .single()
        .unwrap_or(dt);
    start.timestamp_millis()
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
