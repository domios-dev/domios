import { describeSchedule, formatTime, formatFuture } from "./formatters";

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
assert("weekly monday", describeSchedule("0 9 * * 1"), "Every Mon at 9:00");
assert("passthrough complex", describeSchedule("0 9 1 * *"), "0 9 1 * *");
assert("passthrough invalid", describeSchedule("not a cron"), "not a cron");

// Unsupported expressions retain their original text.
assert("weekday range passthrough", describeSchedule("0 9 * * 1-5"), "0 9 * * 1-5");
assert("stepped minute at a fixed hour passthrough", describeSchedule("*/15 9 * * *"), "*/15 9 * * *");
assert("zero step passthrough", describeSchedule("*/0 * * * *"), "*/0 * * * *");
assert("non-numeric step passthrough", describeSchedule("*/x * * * *"), "*/x * * * *");
assert("minute out of range passthrough", describeSchedule("60 9 * * *"), "60 9 * * *");
assert("whitespace passthrough retains raw", describeSchedule("  0 9 * * 1-5  "), "  0 9 * * 1-5  ");

// ── formatTime ────────────────────────────────────────────────────

console.log("\nformatTime:");
const ENGLISH_STAMP = /^[A-Z][a-z]{2} \d{1,2}, \d{2}:\d{2}\s(AM|PM)$/;
const ts = new Date(2025, 0, 15, 10, 30).getTime();
assert("formatTime uses the English UI locale", String(ENGLISH_STAMP.test(formatTime(ts))), "true");

// ── one-shot schedules and formatFuture ───────────────────────────

console.log("\none-shot schedules:");
const oneShot = 1792650940309;
assert("padded epoch-ms schedule reads as a time", describeSchedule(` ${oneShot} `), `at ${formatTime(oneShot)}`);
assert("short numeric schedule passthrough", describeSchedule("179265094030"), "179265094030");
assert("formatFuture tomorrow uses the English UI locale", String(/^tomorrow \d{2}:\d{2}\s(AM|PM)$/.test(formatFuture(Date.now() + 25 * 3_600_000))), "true");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
