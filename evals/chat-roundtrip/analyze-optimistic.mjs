// Diagnosis bridge: feed the REAL transcript captures from run.sh through the
// SAME matchedOptimisticEchoes the chat UI uses, and report whether each real
// echo clears its optimistic bubble — and the seconds-vs-ms skew that decides it.
//
// Run after run.sh has produced target-check/evals/chat-roundtrip/roundtrip-*.json:
//   npx tsx evals/chat-roundtrip/analyze-optimistic.mjs
//
// This is how Part B was diagnosed: it proves, on live-captured data, that a
// fresh echo whose provider timestamp is in Unix seconds would be rejected by
// the ECHO_SKEW guard (read as 1970) unless the matcher normalizes the unit.
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { matchedOptimisticEchoes } from "../../packages/chat-core/src/dedupe.ts";

const here = dirname(fileURLToPath(import.meta.url));
const capDir = join(here, "../../target-check/evals/chat-roundtrip");

function toMs(ts) {
  if (ts == null) return null;
  const n = Number(ts);
  if (Number.isFinite(n)) return n < 1e12 ? n * 1000 : n;
  const p = Date.parse(String(ts));
  return Number.isNaN(p) ? null : p;
}

let files;
try {
  files = readdirSync(capDir).filter((f) => f.startsWith("roundtrip-") && f.endsWith(".json"));
} catch {
  console.error(`no captures in ${capDir} — run ./run.sh first`);
  process.exit(1);
}
if (files.length === 0) {
  console.error(`no roundtrip-*.json captures — run ./run.sh first`);
  process.exit(1);
}

let stuckUnderNaiveGuard = 0;
for (const f of files) {
  const c = JSON.parse(readFileSync(join(capDir, f), "utf8"));
  const provider = f.replace(/^roundtrip-|\.json$/g, "");
  const echo = { id: "opt-1", content: c.user_echo_content, sentAtMs: c.client_send_ms };
  const parsedUser = {
    id: "u-0",
    type: "user",
    content: c.user_echo_content,
    timestamp: c.user_echo_timestamp ?? undefined,
  };
  const { matchedIds } = matchedOptimisticEchoes([echo], [parsedUser]);
  const cleared = matchedIds.includes("opt-1");
  const echoMs = toMs(c.user_echo_timestamp);
  const skew = echoMs == null ? "null-ts" : `${echoMs - c.client_send_ms >= 0 ? "+" : ""}${echoMs - c.client_send_ms}ms`;

  // What a naive ms-only guard (the pre-fix timestampMs) would have done with a
  // raw numeric timestamp: treat the number as ms verbatim.
  const rawNum = Number(c.user_echo_timestamp);
  const naiveMs = Number.isFinite(rawNum) ? rawNum : echoMs;
  const naiveRejects =
    naiveMs != null && naiveMs < c.client_send_ms - 2000 && c.user_echo_timestamp != null;
  if (naiveRejects) stuckUnderNaiveGuard++;

  console.log(
    `${provider.padEnd(8)} cleared=${cleared ? "YES" : "NO "} skew=${skew.padStart(9)} ` +
      `raw_ts=${JSON.stringify(c.user_echo_timestamp)} naive_ms_guard_rejects=${naiveRejects}`,
  );
  if (!cleared) {
    console.error(`  ✗ ${provider}: real echo did NOT clear its bubble — investigate`);
    process.exitCode = 1;
  }
}
console.log(
  `\n${files.length} provider capture(s) analyzed; ${stuckUnderNaiveGuard} would stick on "sending…" under a naive ms-only guard.`,
);
