import { describeSchedule, timeAgo, formatTime } from "./formatters";

let passed = 0;
let failed = 0;

function assert(label: string, actual: string, expected: string) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected "${expected}", got "${actual}"`);
    failed++;
  }
}

// ── describeSchedule ──────────────────────────────────────────────

console.log("describeSchedule:");
assert("every minute", describeSchedule("* * * * *"), "Every minute");
assert("every hour", describeSchedule("0 * * * *"), "Every hour");
assert("every 5 min", describeSchedule("*/5 * * * *"), "Every 5 minutes");
assert("every 2 hours", describeSchedule("0 */2 * * *"), "Every 2 hours at :00");
assert("daily at 9:00", describeSchedule("0 9 * * *"), "Daily at 9:00");
assert("daily at 14:30", describeSchedule("30 14 * * *"), "Daily at 14:30");
assert("weekly monday", describeSchedule("0 9 * * 1"), "Every Mon at 9:00");
assert("weekly sunday", describeSchedule("0 10 * * 0"), "Every Sun at 10:00");
assert("passthrough complex", describeSchedule("0 9 1 * *"), "0 9 1 * *");
assert("passthrough invalid", describeSchedule("not a cron"), "not a cron");

// ── timeAgo ───────────────────────────────────────────────────────

console.log("\ntimeAgo:");
const now = Date.now();
assert("just now", timeAgo(now), "just now");
assert("30s ago", timeAgo(now - 30_000), "just now");
assert("2m ago", timeAgo(now - 120_000), "2m ago");
assert("59m ago", timeAgo(now - 59 * 60_000), "59m ago");
assert("1h ago", timeAgo(now - 3_600_000), "1h ago");
assert("23h ago", timeAgo(now - 23 * 3_600_000), "23h ago");
assert("1d ago", timeAgo(now - 86_400_000), "1d ago");
assert("7d ago", timeAgo(now - 7 * 86_400_000), "7d ago");

// ── formatTime ────────────────────────────────────────────────────

console.log("\nformatTime:");
const ts = new Date(2025, 0, 15, 10, 30).getTime();
const result = formatTime(ts);
// Just verify it returns a non-empty string (locale-dependent)
if (result && result.length > 0) {
  console.log(`  ✓ formatTime returns "${result}"`);
  passed++;
} else {
  console.error(`  ✗ formatTime returned empty`);
  failed++;
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
