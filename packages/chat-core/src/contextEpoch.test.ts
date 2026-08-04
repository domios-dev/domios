import {
  CONTEXT_EPOCH_BOUNDARY_TEXT,
  projectContextEpochs,
} from "./contextEpoch";
import type { NormalizedChatMessage } from "./types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const transcript: NormalizedChatMessage[] = [
  { id: "u0", type: "user", content: "old question", seq: 0 },
  { id: "a0", type: "assistant", content: "old answer", seq: 1 },
  { id: "reset", type: "local_command", content: "/clear", seq: 2 },
  { id: "u1", type: "user", content: "new question", seq: 3 },
];

const firstProjection = projectContextEpochs(transcript);
const dividers = firstProjection.filter((message) => message.systemSubtype === "context_epoch_boundary");
assert(dividers.length === 1, "one reset adds exactly one context-epoch divider");
assert(dividers[0]?.content === CONTEXT_EPOCH_BOUNDARY_TEXT, "divider explains provider exclusion");
assert(firstProjection.some((message) => message.id === "u0"), "prior transcript remains archived and visible");
assert(firstProjection.find((message) => message.id === "u0")?.contextEpoch === 0, "old turns remain in epoch zero");
assert(firstProjection.find((message) => message.id === "u1")?.contextEpoch === 1, "later turns use the new epoch");

const reloadedProjection = projectContextEpochs(transcript.map((message) => ({ ...message })));
assert(
  JSON.stringify(reloadedProjection) === JSON.stringify(firstProjection),
  "reload reconstructs the same persisted transcript epochs and divider",
);
const persistedProjection = projectContextEpochs([
  ...transcript.slice(0, 3),
  { id: "reset-meta", type: "system", content: "reset acknowledged" },
  {
    id: "persisted-boundary",
    type: "system",
    content: CONTEXT_EPOCH_BOUNDARY_TEXT,
    systemSubtype: "context_epoch_boundary",
  },
  transcript[3],
]);
assert(
  persistedProjection.filter((message) => message.systemSubtype === "context_epoch_boundary").length === 1,
  "persisted reload boundary is not duplicated",
);
assert(
  !reloadedProjection.some((message) => /delivery|retry/i.test(message.systemSubtype ?? "")),
  "context epochs add no delivery-status surface",
);

console.log("contextEpoch tests passed");
