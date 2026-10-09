import assert from "node:assert/strict";
import { blueprintModel, nodeKeys, wireId } from "./blueprintModel";
import { followIdentity, typedGraph, watchIdentity } from "./blueprintModel.testdata";
import { nextRunIndex, replayFrame, replaySteps, REPLAY_STEP_MS, runRow, runTimeline, runTransitions, timelineText } from "./blueprintRunModel";
import { clampLogHeight, dragLogHeight, keyLogHeight, LOG_DEFAULT_HEIGHT, LOG_HEIGHT_KEY, LOG_MIN_HEIGHT, readLogHeight, writeLogHeight } from "./dividerModel";
import { graphRuns } from "./blueprintRunModel";
import { automationKey, type AutomationGraph } from "./types";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) { tests.push([name, run]); }
const watch = automationKey(watchIdentity);
const follow = automationKey(followIdentity);

/** One causal run: the watch decides flaky and activates the follow stage, which decides fixed. */
function ranGraph(): AutomationGraph {
  const graph = typedGraph();
  const decision = { decided_by: "adapter" as const, adapter_id: "typesafe-jev", model: "jev-latest", confidence: 0.87, reason: "network timeout in the log" };
  const run = { id: "run-1", root_node: watchIdentity.id, root_transition: "t1", started_at: 1_000, parent: null };
  const [first, second, ...rest] = graph.nodes;
  const watchNode = { ...first, history: [{ ...decision, id: "t1", bucket: "flaky", accepted_at: 1_000, causal_run: run, handlers: [
    { handler_id: "follow-up", status: "completed", execution_id: "a", accepted_at: 1_100 },
    { handler_id: "escalate", status: "skipped", execution_id: null, accepted_at: 1_100 },
  ] }] };
  const followNode = { ...second, history: [{ id: "t2", bucket: "fixed", accepted_at: 1_500, causal_run: { ...run, parent: { node: watchIdentity.id, transition: "t1", handler: "follow-up" } }, handlers: [
    { handler_id: "loop-back", status: "failed", execution_id: "b", accepted_at: 1_600, command_result: null },
  ] }] };
  graph.nodes = [watchNode, followNode, ...rest];
  graph.runs = [{ id: "run-1", root_node: watchIdentity.id, root_transition: "t1", started_at: 1_000, state: "completed", incomplete: false, transitions: [
    { node: watchIdentity, transition_id: "t1", parent: null },
    { node: followIdentity, transition_id: "t2", parent: { node: watchIdentity.id, transition: "t1", handler: "follow-up" } },
  ] }];
  return graph;
}

test("a run groups into stage transitions and replays the typed path hop by hop across the chain", () => {
  const graph = ranGraph();
  const model = blueprintModel(graph);
  const [run] = graphRuns(graph);
  assert.deepEqual(runTransitions(run).map((entry) => [entry.stage, entry.bucket, entry.events.length]), [[watch, "flaky", 2], [follow, "fixed", 1]]);
  const steps = replaySteps(model, run);
  const a = nodeKeys(watch);
  const b = nodeKeys(follow);
  const followUp = a.action("flaky", "follow-up");
  assert.deepEqual(steps, [
    { nodes: [a.trigger], wires: [] },
    { nodes: [a.source], wires: [wireId(a.trigger, "then", a.source)] },
    { nodes: [a.decision], wires: [wireId(a.source, "then", a.decision), wireId(a.source, "output", a.decision)] },
    { nodes: [followUp], wires: [wireId(a.decision, "bucket:flaky", followUp)] },
    { nodes: [b.trigger], wires: [wireId(followUp, "then", b.trigger)] },
    { nodes: [b.source], wires: [wireId(b.trigger, "then", b.source)] },
    { nodes: [b.decision], wires: [wireId(b.source, "then", b.decision), wireId(b.source, "output", b.decision)] },
    { nodes: [b.action("fixed", "loop-back")], wires: [wireId(b.decision, "bucket:fixed", b.action("fixed", "loop-back"))] },
  ], "skipped handlers stay dark");
  const ids = new Set(model.wires.map((wire) => wire.id));
  for (const step of steps) for (const wire of step.wires) assert.ok(ids.has(wire), `${wire} exists in the model`);
});

test("replay frames light the path progressively, travel along the current hop, and settle", () => {
  const steps = [{ nodes: ["a"], wires: [] }, { nodes: ["b"], wires: ["ab"] }, { nodes: ["c"], wires: ["bc"] }];
  const start = replayFrame(steps, 0, false);
  assert.deepEqual([...start.nodes], []);
  assert.equal(start.done, false);
  const mid = replayFrame(steps, REPLAY_STEP_MS * 1.5, false);
  assert.deepEqual([...mid.nodes], ["a"]);
  assert.deepEqual([...mid.wires], ["ab"]);
  assert.deepEqual(mid.travelling, ["ab"]);
  assert.equal(mid.progress, 0.5);
  assert.equal(mid.overall, 0.5);
  const end = replayFrame(steps, REPLAY_STEP_MS * 3, false);
  assert.equal(end.done, true);
  assert.deepEqual([...end.nodes], ["a", "b", "c"]);
  assert.deepEqual(end.travelling, []);
  const reduced = replayFrame(steps, 0, true);
  assert.equal(reduced.done, true, "reduced motion shows the whole path at once");
  assert.deepEqual([...reduced.wires], ["ab", "bc"]);
});

test("pattern transitions keep their origin in the row and timeline with a configured adapter", () => {
  const graph = ranGraph();
  graph.configured_fallback = { adapter_id: "typesafe-jev", model: "jev-latest" };
  const root = graph.nodes[0];
  graph.nodes = [{ ...root, history: root.history!.map((entry) => ({ ...entry, decided_by: "pattern" })) }, ...graph.nodes.slice(1)];
  const run = graphRuns(graph)[0];
  const model = blueprintModel(graph);
  assert.ok(runRow(model, run).path.some((entry) => entry.startsWith("Pattern rules:")));
  assert.ok(runTimeline(model, run).some((entry) => entry.kind === "decision" && entry.title.startsWith("Pattern rules chose")));
  assert.ok(!runRow(model, run).path.some((entry) => entry.includes("Jev")));
  assert.ok(!runTimeline(model, run).some((entry) => entry.kind === "decision" && entry.title.includes("Jev")));
});

test("historical handlers do not light changed routes", () => {
  const graph = ranGraph();
  graph.nodes = graph.nodes.map((node) => ({ ...node, history: node.history?.map((transition) => ({ ...transition, routes_current: false })) }));
  const model = blueprintModel(graph);
  const steps = replaySteps(model, graphRuns(graph)[0]);
  assert.ok(steps.length > 0);
  assert.ok(!steps.flatMap((step) => step.nodes).some((key) => model.nodes.find((node) => node.key === key)?.type === "action"));
  assert.ok(!steps.flatMap((step) => step.wires).some((id) => model.wires.find((wire) => wire.id === id)?.handlerId));
});

test("notification replay keeps its cadence in the shared engine", () => {
  const steps = [{ nodes: [], wires: ["first"] }, { nodes: [], wires: ["second"] }];
  assert.deepEqual(replayFrame(steps, 0, false, 850).travelling, ["first"]);
  assert.deepEqual(replayFrame(steps, 850, false, 850).travelling, ["second"]);
  assert.equal(replayFrame(steps, 1700, false, 850).done, true);
  assert.deepEqual(replayFrame(steps, 0, true, 850).travelling, []);
});

test("a run row summarizes trigger, path, confidence, duration and status", () => {
  const graph = ranGraph();
  const row = runRow(blueprintModel(graph), graphRuns(graph)[0]);
  assert.equal(row.trigger, "Every 20 s");
  assert.deepEqual(row.path, ["Run probe", "Jev · jev-latest: flaky", "Run Follow terminal", "+1 more stage"]);
  assert.equal(row.confidence, 0.87);
  assert.equal(row.durationMs, 600);
  assert.equal(row.status, "failed", "a failed action marks the run failed");
});

test("the timeline reads trigger, source, decision with reason, then each action result", () => {
  const graph = ranGraph();
  const model = blueprintModel(graph);
  const run = graphRuns(graph)[0];
  const entries = runTimeline(model, run);
  assert.deepEqual(entries.slice(0, 5).map((entry) => [entry.kind, entry.title, entry.detail, entry.status]), [
    ["trigger", "Every 20 s", "Build watch", null],
    ["source", "Run probe", "Shell command", null],
    ["decision", "Jev · jev-latest chose flaky · 87% confident", "network timeout in the log", null],
    ["action", "Run Follow terminal", null, "completed"],
    ["action", "Escalate to creator", null, "skipped"],
  ]);
  const text = timelineText(runRow(model, run), entries, () => "14:11");
  assert.ok(text.startsWith("14:11 failed · Every 20 s · Run probe → Jev · jev-latest: flaky"));
  assert.equal(text.split("\n").length, entries.length + 1);
});

test("run list keyboard moves clamp at both ends", () => {
  assert.equal(nextRunIndex(0, 3, "ArrowDown"), 1);
  assert.equal(nextRunIndex(2, 3, "ArrowDown"), 2);
  assert.equal(nextRunIndex(0, 3, "ArrowUp"), 0);
  assert.equal(nextRunIndex(1, 3, "End"), 2);
  assert.equal(nextRunIndex(1, 3, "Home"), 0);
  assert.equal(nextRunIndex(1, 3, "KeyA"), null);
  assert.equal(nextRunIndex(0, 0, "ArrowDown"), null);
});

test("the divider drags, steps by keyboard, clamps to both panes and remembers its height", () => {
  assert.equal(dragLogHeight(260, 500, 440, 800), 320, "dragging up grows the log");
  assert.equal(dragLogHeight(260, 500, 900, 800), LOG_MIN_HEIGHT);
  assert.equal(clampLogHeight(2000, 800), 800 - 180, "the graph keeps its minimum");
  assert.equal(keyLogHeight(260, "ArrowUp", 800), 276);
  assert.equal(keyLogHeight(260, "PageDown", 800), 164);
  assert.equal(keyLogHeight(260, "Home", 800), LOG_MIN_HEIGHT);
  assert.equal(keyLogHeight(260, "End", 800), 620);
  assert.equal(keyLogHeight(260, "Enter", 800), null);
  const store = new Map<string, string>();
  const storage = { getItem: (key: string) => store.get(key) ?? null, setItem: (key: string, value: string) => { store.set(key, value); } };
  assert.equal(readLogHeight(storage), LOG_DEFAULT_HEIGHT);
  writeLogHeight(storage, 333.4);
  assert.equal(store.get(LOG_HEIGHT_KEY), "333");
  assert.equal(readLogHeight(storage), 333);
  assert.equal(readLogHeight({ getItem: () => { throw new Error("blocked"); } }), LOG_DEFAULT_HEIGHT);
  assert.doesNotThrow(() => writeLogHeight({ setItem: () => { throw new Error("blocked"); } }, 300));
});

let failed = false;
for (const [name, run] of tests) {
  try { run(); console.log(`✓ ${name}`); } catch (error) { failed = true; console.error(`✗ ${name}`); console.error(error); }
}
if (failed) process.exitCode = 1;
else console.log(`${tests.length} blueprint run cases passed`);
