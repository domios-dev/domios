// Run: npx tsx packages/chat-core/src/timeline.test.ts
import { mergeTimelineMessages } from "./timeline";
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

function msg(id: string, type: NormalizedChatMessage["type"], ts?: number): NormalizedChatMessage {
  return {
    id,
    type,
    content: id,
    timestamp: ts == null ? undefined : new Date(ts).toISOString(),
  };
}

const T0 = 1_700_000_000_000;

{
  const merged = mergeTimelineMessages(
    [msg("assistant-after-send", "assistant", T0 + 1_000)],
    [msg("pending-user", "user", T0)],
  );
  eq(
    merged.map((m) => m.id),
    ["pending-user", "assistant-after-send"],
    "pending user stays in timeline before later assistant replies",
  );
}

{
  const merged = mergeTimelineMessages(
    [msg("old", "assistant", T0), msg("new", "assistant", T0 + 2_000)],
    [msg("middle", "user", T0 + 1_000)],
  );
  eq(
    merged.map((m) => m.id),
    ["old", "middle", "new"],
    "pending messages insert between confirmed messages by timestamp",
  );
}

{
  const merged = mergeTimelineMessages(
    [msg("confirmed-a", "assistant"), msg("confirmed-b", "assistant")],
    [msg("pending", "user")],
  );
  eq(
    merged.map((m) => m.id),
    ["confirmed-a", "confirmed-b", "pending"],
    "missing timestamps keep stable source order",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
