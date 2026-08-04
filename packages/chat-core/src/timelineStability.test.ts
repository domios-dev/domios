import { projectContextEpochs } from "./contextEpoch";
import { reduceToolEvents } from "./toolEvents";
import { stabilizeTimelineItems } from "./timelineStability";
import type { NormalizedChatMessage } from "./types";

function assert(value: unknown, label: string): asserts value {
  if (!value) throw new Error(label);
}

function call(id: string, toolId: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return { id, type: "tool_call", content: "", toolId, toolKind: "generic", toolSummary: "run", ...patch };
}

function result(id: string, toolId: string, detail: string, patch: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage {
  return { id, type: "tool_result", content: "", toolId, toolKind: "generic", toolDetail: detail, ...patch };
}

function assistant(id: string, content: string): NormalizedChatMessage {
  return { id, type: "assistant", content };
}

// --- unchanged tool items keep object identity across reduce passes ---
{
  const base = [assistant("a-1", "hello"), call("c-1", "tool-1"), result("r-1", "tool-1", "done")];
  const first = stabilizeTimelineItems(undefined, reduceToolEvents(base));
  const second = stabilizeTimelineItems(first, reduceToolEvents([...base, assistant("a-2", "streaming…")]));

  assert(second.length === 3, "second pass has message + merged tool + new message");
  assert(second[0] === first[0], "unchanged message wrapper keeps identity");
  assert(second[1] === first[1], "unchanged merged tool item keeps identity");
  assert(second[1].kind === "tool", "merged tool item stays a tool item");
  assert(second[2] !== first[1], "appended message is a fresh item");
}

// --- streaming updates to the streamed message do not disturb other items ---
{
  const streaming = assistant("a-2", "partial");
  const base = [call("c-1", "tool-1"), result("r-1", "tool-1", "done"), streaming];
  const first = stabilizeTimelineItems(undefined, reduceToolEvents(base));
  const grown = [...base.slice(0, 2), { ...streaming, content: "partial plus more" }];
  const second = stabilizeTimelineItems(first, reduceToolEvents(grown));

  assert(second[0] === first[0], "tool item untouched by the streaming tail keeps identity");
  assert(second[1] !== first[1], "the streamed message gets a fresh wrapper");
}

// --- a newly arrived result invalidates only its own tool item ---
{
  const base = [call("c-1", "tool-1"), call("c-2", "tool-2"), result("r-1", "tool-1", "done")];
  const first = stabilizeTimelineItems(undefined, reduceToolEvents(base));
  const second = stabilizeTimelineItems(
    first,
    reduceToolEvents([...base, result("r-2", "tool-2", "finished")]),
  );

  assert(second[0] === first[0], "tool-1 stays identical when tool-2 resolves");
  assert(second[1] !== first[1], "tool-2 item is replaced when its result lands");
  assert(second[1].kind === "tool" && second[1].event.status === "success", "tool-2 shows its merged result");
}

// --- wait batches stay stable while their members are unchanged ---
{
  const waits = [
    call("w-1", "wait-1", { toolName: "wait", timestamp: "1000", sourceSessionId: "s" }),
    call("w-2", "wait-2", { toolName: "wait", timestamp: "2000", sourceSessionId: "s" }),
  ];
  const first = stabilizeTimelineItems(undefined, reduceToolEvents(waits));
  const second = stabilizeTimelineItems(first, reduceToolEvents([...waits, assistant("a-1", "after")]));

  assert(first.length === 1 && first[0].kind === "tool" && first[0].event.waitBatch, "waits coalesce into one batch");
  assert(second[0] === first[0], "unchanged wait batch keeps identity");
}

// --- error results propagate and refresh the item ---
{
  const base = [call("c-1", "tool-1")];
  const first = stabilizeTimelineItems(undefined, reduceToolEvents(base));
  const second = stabilizeTimelineItems(
    first,
    reduceToolEvents([...base, result("r-1", "tool-1", "boom", { toolError: true })]),
  );
  assert(second[0] !== first[0], "errored tool item is replaced");
  assert(second[0].kind === "tool" && second[0].event.status === "error", "error status propagates");
}

// --- re-materialized messages recover identity (the real streaming path) ---
// The chat view runs projectContextEpochs before reducing, and that stamps
// contextEpoch onto every message, so each pass hands stabilize a fresh object
// for rows that never changed. Comparing by reference alone made every bubble
// re-render once per streamed chunk.
{
  const stable = [assistant("a-1", "one"), assistant("a-2", "two")];
  const project = (tail: NormalizedChatMessage) =>
    reduceToolEvents(projectContextEpochs([...stable, tail]));

  const first = stabilizeTimelineItems(undefined, project(assistant("tail", "stream")));
  const second = stabilizeTimelineItems(first, project(assistant("tail", "streamxx")));

  assert(second[0] === first[0], "re-materialized unchanged message keeps identity");
  assert(second[1] === first[1], "every unchanged row keeps identity, not just the first");
  assert(second[2] !== first[2], "the streamed tail still gets a fresh wrapper");
}

// --- an equivalent-looking message with a changed field is not reused ---
{
  const base = [assistant("a-1", "one")];
  const first = stabilizeTimelineItems(undefined, reduceToolEvents(base));
  const second = stabilizeTimelineItems(
    first,
    reduceToolEvents([{ ...base[0], timestamp: "42" }]),
  );
  assert(second[0] !== first[0], "a new field on the message invalidates the wrapper");
}

console.log("timeline stability checks passed");
