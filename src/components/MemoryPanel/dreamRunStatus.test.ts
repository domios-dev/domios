import type { GraphWorkerInfo } from "../../lib/types";
import { diaryCompleteness, diaryRecordState, dreamRunLabel } from "./dreamRunStatus";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

// daily_dream_2026_09_11_08_30 as still stored after its final update timed out.
const UNFINISHED = [
  "<insight>WIP: dreaming...</insight>",
  "<log>",
  "STARTED: 2026-09-11T08:30:00Z",
  "PATTERN: pending",
  "HUBS: pending",
  "CALIBRATION: pending",
  "ACTION: pending",
  "</log>",
].join("\n");

const FINISHED = [
  "<insight>The graph keeps rebuilding the same three bridges every night.</insight>",
  "<log>",
  "PATTERN: repeat bridging between mem and mesh clusters",
  "HUBS_AFTER: 1",
  "CALIBRATION: swept 412 items",
  "</log>",
].join("\n");

function worker(overrides: Partial<GraphWorkerInfo>): GraphWorkerInfo {
  return {
    worker_id: "gw_test0001",
    tab_id: "tab1",
    intent: "dreaming:rem",
    spawned_by: "system:dreaming",
    spawned_at: 1_789_183_000_000,
    status: "done",
    progress_text: null,
    touched_nodes: [],
    color: "hsl(0,0%,0%)",
    report: null,
    done_at: 1_789_184_113_893,
    ...overrides,
  } as GraphWorkerInfo;
}

const unfinished = diaryCompleteness(UNFINISHED);
assert(!unfinished.complete, "placeholder diary must read as incomplete");
assert(unfinished.placeholderInsight, "skeleton insight must be detected");
assert(
  unfinished.pendingFields.join(",") === "PATTERN,HUBS,CALIBRATION,ACTION",
  `unexpected pending fields: ${unfinished.pendingFields.join(",")}`,
);

const finished = diaryCompleteness(FINISHED);
assert(finished.complete, "a filled diary must read as complete");

// Prose never decides liveness: same body, two different worker facts.
const live = diaryRecordState("daily_dream_2026_09_11_08_30", UNFINISHED, [
  worker({ worker_id: "gw_live0001", status: "writing", diary_id: "daily_dream_2026_09_11_08_30" }),
]);
assert(live.state === "incomplete_live", `expected live owner, got ${live.state}`);

const historical = diaryRecordState("daily_dream_2026_09_11_08_30", UNFINISHED, [
  worker({ worker_id: "gw_7b948eb1", status: "done", diary_id: "daily_dream_2026_09_11_08_30" }),
]);
assert(
  historical.state === "incomplete_recoverable",
  `expected recoverable history, got ${historical.state}`,
);
assert(
  historical.state === "incomplete_recoverable" && historical.reason.includes("PATTERN"),
  "recoverable reason must name the pending fields",
);

// A reported run over an unverified diary is not a completion.
const unverified = dreamRunLabel(worker({ status: "done" }));
assert(unverified.label === "unverified", `expected unverified, got ${unverified.label}`);

const partial = dreamRunLabel(
  worker({
    status: "done",
    finalization: {
      state: "partial",
      diary_id: "daily_dream_2026_09_11_08_30",
      reason: "final diary update/self-link timed out",
    },
  }),
);
assert(partial.label === "partial", `expected partial, got ${partial.label}`);
assert(partial.reason?.includes("timed out") === true, "partial must carry its reason");

const finalized = dreamRunLabel(
  worker({
    status: "done",
    finalization: { state: "succeeded", diary_id: "daily_dream_2026_09_12_03_20" },
  }),
);
assert(finalized.label === "done", `expected done, got ${finalized.label}`);

const running = dreamRunLabel(worker({ status: "writing", report: "WIP: dreaming..." }));
assert(running.label === "running", "a non-terminal worker is the only source of running");

const endedWithWipProse = dreamRunLabel(worker({ status: "error", report: "WIP: dreaming..." }));
assert(
  endedWithWipProse.label !== "running",
  "WIP prose in a report must not keep a finished run looking live",
);

console.log("✓ dream run status separates execution end from diary finalization");
