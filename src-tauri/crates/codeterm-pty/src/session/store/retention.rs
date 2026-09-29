use std::collections::HashSet;

const RECENT_WINDOW_SECS: u64 = 10 * 60;
const RECENT_MAX: usize = 60;
const MINUTE_WINDOW_SECS: u64 = 60 * 60;
const HOUR_WINDOW_SECS: u64 = 24 * 60 * 60;
const DAY_WINDOW_SECS: u64 = 7 * 24 * 60 * 60;

/// Backup timestamps (unix secs) to delete: every snapshot of the last 10 min
/// (capped), then one per minute for an hour, one per hour for a day, one per
/// day for a week. Everything older is dropped.
pub(super) fn timestamps_to_evict(timestamps: &[u64], now: u64) -> Vec<u64> {
    let mut newest_first = timestamps.to_vec();
    newest_first.sort_unstable_by(|a, b| b.cmp(a));
    newest_first.dedup();

    let mut kept_buckets: HashSet<(u8, u64)> = HashSet::new();
    let mut recent_kept = 0usize;
    let mut evict = Vec::new();
    for ts in newest_first {
        let age = now.saturating_sub(ts);
        let keep = if age < RECENT_WINDOW_SECS {
            recent_kept += 1;
            recent_kept <= RECENT_MAX
        } else if age < MINUTE_WINDOW_SECS {
            kept_buckets.insert((1, ts / 60))
        } else if age < HOUR_WINDOW_SECS {
            kept_buckets.insert((2, ts / 3600))
        } else if age < DAY_WINDOW_SECS {
            kept_buckets.insert((3, ts / 86_400))
        } else {
            false
        };
        if !keep {
            evict.push(ts);
        }
    }
    evict
}
