import { timestampMs } from "./time";
import { chronologicalOrder, inheritedOrderKeys } from "./timeline";
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

function msg(
  id: string,
  type: NormalizedChatMessage["type"],
  ts?: number,
  extra: Partial<NormalizedChatMessage> = {},
): NormalizedChatMessage {
  return {
    id,
    type,
    content: id,
    timestamp: ts == null ? undefined : new Date(ts).toISOString(),
    ...extra,
  };
}

const T0 = 1_700_000_000_000;
const ownKeys = (rows: NormalizedChatMessage[]) => inheritedOrderKeys(rows, (row) => timestampMs(row.timestamp));
const ordered = (rows: NormalizedChatMessage[]) => chronologicalOrder(rows, ownKeys(rows)).map((m) => m.id);

{
  const rows = [
    msg("older", "assistant", T0),
    msg("text-after-send", "assistant", T0 + 2_000),
    msg("tool-after-send", "tool_call", T0 + 3_000),
    msg("result-after-send", "tool_result", T0 + 4_000),
    msg("consumed-user", "user", T0 + 1_000, { timelineMarker: { kind: "queued" } }),
    msg("reply", "assistant", T0 + 5_000),
  ];
  eq(
    ordered(rows),
    ["older", "consumed-user", "text-after-send", "tool-after-send", "result-after-send", "reply"],
    "a user row recorded after later output sits at its own event time",
  );
}

{
  const rows = [msg("a", "assistant", T0), msg("u", "user", T0 + 1_000), msg("b", "assistant", T0 + 2_000)];
  eq(chronologicalOrder(rows, ownKeys(rows)) === rows, true, "an already chronological transcript keeps its identity");
}

{
  const rows = [
    msg("first-written-user", "user", T0 + 2_000),
    msg("late-assistant", "assistant", T0 + 3_000),
    msg("earlier-sent-user", "user", T0 + 1_000),
  ];
  eq(
    ordered(rows),
    ["earlier-sent-user", "first-written-user", "late-assistant"],
    "user rows order by their own keys, whichever was written first",
  );
}

{
  const rows = [
    msg("assistant-b", "assistant", T0 + 2_000),
    msg("assistant-a", "assistant", T0 + 1_000),
  ];
  eq(ordered(rows), ["assistant-a", "assistant-b"], "provider timestamps order every row kind, not only user rows");
}

{
  const rows = [
    msg("same-second-first", "assistant", T0),
    msg("same-second-second", "assistant", T0),
  ];
  eq(ordered(rows), ["same-second-first", "same-second-second"], "equal keys keep source order");
}

{
  const rows = [
    msg("stamped", "assistant", T0 + 3_000),
    msg("unstamped", "assistant"),
    msg("late", "assistant", T0 + 4_000),
    msg("user", "user", T0 + 1_000),
  ];
  eq(ownKeys(rows)[1], null, "a row without a time or event parent never borrows a neighbour's key");
  eq(
    ordered(rows),
    ["user", "unstamped", "stamped", "late"],
    "a keyless row holds its position and blocks no keyed row from reordering",
  );
}

{
  const rows = [
    msg("call", "tool_call", T0 + 5_000, { toolId: "toolu_1" }),
    msg("unrelated", "assistant", T0 + 6_000),
    msg("result", "tool_result", undefined, { toolId: "toolu_1" }),
    msg("user", "user", T0 + 5_500),
  ];
  eq(ownKeys(rows)[2], T0 + 5_000, "an untimed tool result inherits its call's key");
  eq(ordered(rows), ["call", "result", "user", "unrelated"], "the result travels with its call, not with its file neighbour");
}

{
  const rows = [
    msg("300:0", "assistant", T0 + 7_000, { sourceMessageId: "record-uuid-0" }),
    msg("300:1", "assistant", undefined, { sourceMessageId: "record-uuid-1" }),
    msg("db-1", "assistant", T0 + 8_000),
    msg("db-2", "assistant"),
  ];
  const keys = ownKeys(rows);
  eq(keys[1], T0 + 7_000, "an untimed continuation block inherits its source record's key");
  eq(keys[3], null, "rows sharing only an id prefix are not proven continuations");
}

{
  const rows = [
    msg("100:0", "assistant", T0 + 7_000, { sourceMessageId: "5-0" }),
    msg("200:0", "assistant", undefined, { sourceMessageId: "5-1" }),
  ];
  eq(ownKeys(rows)[1], null, "a stream ordinal reused by a later batch never links its rows as one record");
}

{
  const rows = [
    msg("result-first", "tool_result", undefined, { toolId: "toolu_late" }),
    msg("call-later", "tool_call", T0, { toolId: "toolu_late" }),
  ];
  eq(ownKeys(rows)[0], null, "a result never inherits from a call that appears after it");
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
