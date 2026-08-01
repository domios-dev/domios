// Run: npx tsx packages/chat-core/src/watcherVerdict.test.ts
// Mirrors `src-tauri/src/service/watcher/verdict/tests.rs` parse_* cases.

import { parseWatcherVerdictContent } from "./watcherVerdict";

let passed = 0;
let failed = 0;

function eq(actual: unknown, expected: unknown, name: string) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}: expected ${e}, got ${a}`);
  }
}

eq(
  parseWatcherVerdictContent('{"status":"ok","summary":"all good","actions":[]}'),
  { status: "ok", summary: "all good", actions: [] },
  "parse_naked_json",
);

eq(
  parseWatcherVerdictContent(
    'Here is my verdict:\n```json\n{"status":"stalled","summary":"stuck","actions":[]}\n```\nThanks.',
  ),
  { status: "stalled", summary: "stuck", actions: [] },
  "parse_fenced_json_block",
);

eq(
  parseWatcherVerdictContent(
    'Assessment follows. {"status":"attention","summary":"check workers","actions":[]} End.',
  ),
  { status: "attention", summary: "check workers", actions: [] },
  "parse_json_with_surrounding_prose",
);

const malformedRaw = "This is not JSON at all, just prose.";
eq(
  parseWatcherVerdictContent(malformedRaw),
  { status: "attention", summary: malformedRaw, actions: [] },
  "malformed_degrades_to_attention_without_actions",
);

eq(
  parseWatcherVerdictContent(
    '{"status":"exploded","summary":"weird","actions":[{"kind":"nudge","tab":"p1","message":"hi"}]}',
  ),
  {
    status: "attention",
    summary: "weird",
    actions: [{ kind: "nudge", tab: "p1", message: "hi" }],
  },
  "unknown_status_degrades_to_attention",
);

const missingSummaryRaw = '{"status":"ok","summary":"","actions":[]}';
eq(parseWatcherVerdictContent(missingSummaryRaw).status, "ok", "blank summary keeps valid status");
eq(
  parseWatcherVerdictContent(missingSummaryRaw).summary,
  missingSummaryRaw,
  "blank summary falls back to raw prefix",
);

const noSummaryRaw = '{"status":"ok","actions":[]}';
eq(parseWatcherVerdictContent(noSummaryRaw).summary, noSummaryRaw, "missing summary falls back to raw prefix");

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
