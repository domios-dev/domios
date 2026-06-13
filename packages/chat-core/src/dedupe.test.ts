// Run: npx tsx packages/chat-core/src/dedupe.test.ts
import { matchedOptimisticEchoes, type OptimisticEcho } from "./dedupe";
import type { NormalizedChatMessage } from "./types";

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

function user(id: string, content: string, tsMs: number): NormalizedChatMessage {
  return { id, type: "user", content, timestamp: new Date(tsMs).toISOString() };
}
function echo(id: string, content: string, sentAtMs: number): OptimisticEcho {
  return { id, content, sentAtMs };
}

const T0 = 1_700_000_000_000;

// 1. No real echo yet → bubble stays pending forever (no timer eviction).
//    This is root cause #1: the old 60s TTL deleted it; now only a real echo can.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "deploy the thing", T0)],
    [],
  );
  eq(matchedIds, [], "no echo → not matched");
  eq(pendingIds, ["opt-1"], "no echo → stays pending");
}

// 2. Real echo with equal content AND timestamp >= sentAt → matched (bubble cleared).
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "hello world", T0)],
    [user("uuid-a-0", "hello world", T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "real echo after send → matched");
}

// 3. An OLDER identical message (ts < sentAt) must NOT clear a fresh bubble.
//    Root cause #3: re-sending a short line ("yes") must match the NEW echo,
//    never an identical line from earlier in the history.
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0)],
    [user("old-0", "yes", T0 - 60_000)], // a minute-old "yes" in history
  );
  eq(matchedIds, [], "stale prior identical message → not matched");
  eq(pendingIds, ["opt-1"], "fresh bubble stays pending until its own echo");
}

// 4. Same short message sent twice → two real echoes consume-once → both cleared
//    distinctly. The old loose/substring + content-dedup collapsed these.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "yes", T0), echo("opt-2", "yes", T0 + 10_000)],
    [user("a-0", "yes", T0 + 300), user("b-0", "yes", T0 + 10_300)],
  );
  eq(matchedIds.sort(), ["opt-1", "opt-2"], "two sends → two distinct echoes matched");
}

// 5. Two identical pending bubbles but only ONE real echo → consume-once: exactly
//    one cleared, the other stays pending (no double-clear off a single message).
{
  const { matchedIds, pendingIds } = matchedOptimisticEchoes(
    [echo("opt-1", "continue", T0), echo("opt-2", "continue", T0 + 1_000)],
    [user("a-0", "continue", T0 + 500)],
  );
  eq(matchedIds, ["opt-1"], "single echo clears exactly one bubble (oldest first)");
  eq(pendingIds, ["opt-2"], "second identical bubble still pending");
}

// 6. Content normalization: CRLF / whitespace differences still match.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "line one\r\nline two", T0)],
    [user("a-0", "line one\nline two", T0 + 100)],
  );
  eq(matchedIds, ["opt-1"], "normalized content (CRLF) matches");
}

// 7. A parsed message with no timestamp falls back to content+consume-once match.
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "no ts here", T0)],
    [{ id: "a-0", type: "user", content: "no ts here" }],
  );
  eq(matchedIds, ["opt-1"], "missing timestamp → matched on content");
}

// 8. Sub-second flooring is tolerated by the small skew (echo ts slightly < sentAt).
{
  const { matchedIds } = matchedOptimisticEchoes(
    [echo("opt-1", "edge", T0)],
    [user("a-0", "edge", T0 - 800)], // floored within the 2s grace
  );
  eq(matchedIds, ["opt-1"], "sub-second floored echo within skew → matched");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
