import { workerEmptyState } from "./workerEmptyState";

const tests: Array<[string, () => void]> = [];
const test = (name: string, fn: () => void) => tests.push([name, fn]);
const eq = (a: unknown, b: unknown, msg: string) => {
  if (a !== b) throw new Error(`${msg}: expected ${JSON.stringify(b)}, got ${JSON.stringify(a)}`);
};

test("orphaned worker with no session shows explicit orphaned state", () => {
  eq(workerEmptyState(null, false, "orphaned"), "Worker orphaned — no active session", "orphaned must not say Waiting for spawn");
});

test("error worker shows failure state", () => {
  eq(workerEmptyState(null, false, "error"), "Worker failed", "error must show Worker failed");
});

test("done worker with no log shows completed state", () => {
  eq(workerEmptyState(null, false, "done"), "Completed — no output captured", "done with no log must not say Waiting for spawn");
});

test("done worker report remains available to the detail state", () => {
  eq(workerEmptyState(null, false, "done", "Saved 3 memories"), "Completed — no output captured", "done state must remain completed when a report exists");
});

test("hard timeout shows explicit state and reason", () => {
  eq(workerEmptyState(null, false, "hard_timeout", "hard timeout after 600s"), "Worker hard timeout — hard timeout after 600s", "hard timeout must expose its reason");
});

test("cancelled worker shows explicit cancelled state", () => {
  eq(workerEmptyState(null, false, "cancelled"), "Worker cancelled", "cancelled must not say Waiting for spawn");
});

test("starting worker with no pane_id shows Waiting for spawn", () => {
  eq(workerEmptyState(null, false, "starting"), "Waiting for spawn…", "starting without pane_id is genuinely pending");
});

test("queued worker shows queued state", () => {
  eq(workerEmptyState(null, false, "queued"), "Queued — waiting for worker slot…", "queued must show slot wait message");
});

test("live connected worker shows waiting for first event", () => {
  eq(workerEmptyState("pane-1", true, "reading"), "Waiting for first event…", "live connected must wait for events");
});

test("live disconnected worker shows connecting", () => {
  eq(workerEmptyState("pane-1", false, "analyzing"), "Connecting…", "live disconnected must show connecting");
});

test("case insensitive: ORPHANED matches orphaned", () => {
  eq(workerEmptyState(null, false, "ORPHANED"), "Worker orphaned — no active session", "uppercase ORPHANED must resolve to orphaned state");
});

test("case insensitive: ERROR matches error", () => {
  eq(workerEmptyState(null, false, "ERROR"), "Worker failed", "uppercase ERROR must resolve to error state");
});

async function main() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try { fn(); console.log(`✓ ${name}`); }
    catch (e) { failed += 1; console.error(`✗ ${name}`, e); }
  }
  if (failed) throw new Error(`${failed} agent-run-log-popup test(s) failed`);
}

void main();
