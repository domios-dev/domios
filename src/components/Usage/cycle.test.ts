// Run: npx tsx src/components/Usage/cycle.test.ts

import { nextResetAt, zeroExpiredResets } from "./cycle";
import type { UsageSnapshot } from "../../types/usage";

let failed = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`✓ ${name}`);
  } catch (e) {
    failed += 1;
    console.error(`✗ ${name}`);
    console.error(e);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

const NOW = 1_000_000_000;

function snap(over: Partial<UsageSnapshot>): UsageSnapshot {
  return {
    id: 1,
    provider: "claude",
    account_id: null,
    captured_at: NOW,
    parsed_ok: true,
    billing_mode: "unknown",
    limits_reported: false,
    session_pct: null,
    session_resets_at: null,
    weekly_pct: null,
    weekly_resets_at: null,
    spend_cents: null,
    spend_limit_cents: null,
    spend_remaining_cents: null,
    spend_resets_at: null,
    failure_reason: null,
    probe_command: null,
    ...over,
  };
}

test("nextResetAt picks the soonest future timer", () => {
  const s = snap({ session_resets_at: NOW + 50, weekly_resets_at: NOW + 10 });
  assert(nextResetAt(s, NOW) === NOW + 10, "soonest future reset");
});

test("nextResetAt ignores past/absent timers", () => {
  const s = snap({ session_resets_at: NOW - 5, weekly_resets_at: null });
  assert(nextResetAt(s, NOW) === null, "all past/absent → null");
});

test("nextResetAt considers spend reset", () => {
  const s = snap({ spend_cents: 1250, spend_resets_at: NOW + 99 });
  assert(nextResetAt(s, NOW) === NOW + 99, "spend reset counts");
});

test("zeroExpiredResets zeros only the expired session pct", () => {
  const s = snap({
    session_pct: 92,
    session_resets_at: NOW - 1,
    weekly_pct: 40,
    weekly_resets_at: NOW + 100,
  });
  const out = zeroExpiredResets(s, NOW);
  assert(out !== s, "returns a new object when something expired");
  assert(out.session_pct === 0, "expired session zeroed");
  assert(out.session_resets_at === null, "expired session timer cleared");
  assert(out.weekly_pct === 40, "future weekly untouched");
  assert(out.weekly_resets_at === NOW + 100, "future weekly timer kept");
});

test("zeroExpiredResets zeros spend at cycle rollover", () => {
  const s = snap({ spend_cents: 1917, spend_resets_at: NOW - 1 });
  const out = zeroExpiredResets(s, NOW);
  assert(out.spend_cents === 0, "spend zeroed on rollover");
  assert(out.spend_resets_at === null, "spend timer cleared");
});

test("zeroExpiredResets returns same reference when nothing expired", () => {
  const s = snap({ session_pct: 50, session_resets_at: NOW + 10 });
  assert(zeroExpiredResets(s, NOW) === s, "no-op keeps identity (avoids re-render churn)");
});

test("zeroExpiredResets leaves null pct null (no fabricated zero)", () => {
  const s = snap({ session_pct: null, session_resets_at: NOW - 1 });
  const out = zeroExpiredResets(s, NOW);
  assert(out.session_pct === null, "null stays null");
  assert(out.session_resets_at === null, "timer still cleared");
});

console.log(`\n${failed === 0 ? "all green" : failed + " failed"}`);
if (failed > 0) process.exit(1);
