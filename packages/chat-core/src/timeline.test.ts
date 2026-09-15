import { chronologicalRows, mergeTimelineMessages } from "./timeline";
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
    [
      {
        ...msg("assistant-arrived-after-send-with-skewed-provider-time", "assistant", T0 - 1_000),
        clientOrder: 2,
      },
    ],
    [{ ...msg("pending-user", "user", T0), clientOrder: 1 }],
  );
  eq(
    merged.map((m) => m.id),
    ["pending-user", "assistant-arrived-after-send-with-skewed-provider-time"],
    "live optimistic user keeps client order before later-arriving assistant despite provider timestamp skew",
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

{
  const rows: NormalizedChatMessage[] = [
    msg("older", "assistant", T0),
    msg("text-after-send", "assistant", T0 + 2_000),
    msg("tool-after-send", "tool_call", T0 + 3_000),
    msg("result-after-send", "tool_result", T0 + 4_000),
    { ...msg("consumed-user", "user", T0 + 1_000), timelineMarker: { kind: "queued" } },
    msg("reply", "assistant", T0 + 5_000),
  ];
  eq(
    chronologicalRows(rows).map((m) => m.id),
    ["older", "consumed-user", "text-after-send", "tool-after-send", "result-after-send", "reply"],
    "a user row recorded after later output sits at its own send timestamp as an ordinary row",
  );
}

{
  const rows = [msg("a", "assistant", T0), msg("u", "user", T0 + 1_000), msg("b", "assistant", T0 + 2_000)];
  eq(chronologicalRows(rows) === rows, true, "an already chronological transcript keeps its identity");
}

{
  const rows = [
    msg("first-user", "user", T0 + 2_000),
    msg("late-assistant", "assistant", T0 + 3_000),
    msg("second-user", "user", T0 + 1_000),
  ];
  eq(
    chronologicalRows(rows).map((m) => m.id),
    ["first-user", "second-user", "late-assistant"],
    "a user row never crosses an earlier user row",
  );
}

{
  const rows = [
    msg("stamped", "assistant", T0 + 3_000),
    msg("unstamped", "assistant"),
    msg("late", "assistant", T0 + 4_000),
    msg("user", "user", T0 + 1_000),
  ];
  eq(
    chronologicalRows(rows).map((m) => m.id),
    ["stamped", "unstamped", "user", "late"],
    "a user row never crosses a row without a timestamp",
  );
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
