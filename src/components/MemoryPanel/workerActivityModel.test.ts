import assert from "node:assert/strict";
import type { GraphWorkerInfo } from "../../lib/types";
import { groupRecentWorkers, workerDurationMs, workerTimeLabels } from "./workerActivityModel";

function worker(overrides: Partial<GraphWorkerInfo> = {}): GraphWorkerInfo {
  return {
    worker_id: "w1", tab_id: null, intent: "extract:item", spawned_by: "auto:save",
    spawned_at: 1000, status: "error", progress_text: null, touched_nodes: [], color: "red",
    report: null, done_at: null, agent_provider: "fixture", model: "model",
    execution_end: { kind: "process_ended", detail: "launch refused" }, ...overrides,
  };
}

const recent = groupRecentWorkers([
  worker(), worker({ worker_id: "w2", spawned_at: 2000 }),
  worker({ worker_id: "w3", intent: "extract:other" }),
  worker({ worker_id: "w4", execution_end: { kind: "process_ended", detail: "permission refused" } }),
  worker({ worker_id: "w5", agent_provider: "other" }),
  worker({ worker_id: "w6", status: "done" }), worker({ worker_id: "w7", status: "done" }),
  worker({ worker_id: "live", status: "starting" }),
]);
assert.equal(recent.length, 6);
assert.equal(recent[0].count, 2);
assert.equal(recent[0].worker.worker_id, "w2");
assert.equal(workerDurationMs(worker(), 999_999), null);
assert.equal(workerDurationMs(worker({ status: "orphaned", done_at: 999_999 }), 999_999), null);
assert.equal(workerDurationMs(worker({ status: "orphaned", last_observed_at: 4000, done_at: 999_999 }), 999_999), 3000);
assert.equal(workerDurationMs(worker({ status: "done", done_at: 5000 }), 999_999), 4000);
assert.equal(workerDurationMs(worker({ status: "starting" }), 5000), 4000);
assert.deepEqual(workerTimeLabels(worker(), 61_000), { age: "Age 1m 0s", duration: "Duration unknown" });
assert.equal(workerDurationMs(worker({ done_at: 500 }), 1000), 0);
console.log("✓ Worker failure grouping and honest age/duration semantics");
