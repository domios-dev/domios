import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { blueprintTriggerSummary, buildBlueprintGraph, litWires, nodeLabel, type BlueprintGraphModel } from "./graphModel";
import { layoutBlueprint, ROW_HEIGHT, type BlueprintLayout } from "./layout";
import { watchRuns, withRetainedRuns } from "./runEvidence";
import { blueprintNodeKey, toggleBlueprintExpansion } from "../blueprintExpansion";
import { buildIncrementally } from "../blueprintIncremental";
import { blueprintDiffModel } from "../blueprintDiffModel";
import { blueprintJournalNodes, blueprintPipelineOutcome, heldReasonLabel } from "../blueprintPipelineOutcome";
import { pipelineModel, type PipelineView } from "../pipelineModel";
import { call, document, diff, viewerPipeline } from "../blueprintViewer.testdata";
import capturedWatch from "../blueprintWatchCaptured.testdata.json";
import type { AutomationGraph } from "../types";

const key = blueprintNodeKey;

function assertGeometry(model: BlueprintGraphModel, layout: BlueprintLayout) {
  assert.equal(layout.nodes.length, model.nodes.length, "every node is placed");
  assert.equal(layout.wires.length, model.wires.length, "every wire is routed");
  for (const [index, a] of layout.nodes.entries()) {
    assert(a.box.x >= 0 && a.box.y >= 0 && a.box.x + a.box.width <= layout.bounds.width && a.box.y + a.box.height <= layout.bounds.height, `${a.node.key} inside bounds`);
    for (const b of layout.nodes.slice(index + 1)) {
      const apart = a.box.x + a.box.width <= b.box.x || b.box.x + b.box.width <= a.box.x || a.box.y + a.box.height <= b.box.y || b.box.y + b.box.height <= a.box.y;
      assert(apart, `${a.node.key} overlaps ${b.node.key}`);
    }
  }
  for (const route of layout.wires) {
    const from = layout.byKey.get(route.wire.from.node)!, to = layout.byKey.get(route.wire.to.node)!;
    const out = from.rows.find((row) => row.right?.id === route.wire.from.pin)!, into = to.rows.find((row) => row.left?.id === route.wire.to.pin)!;
    assert.deepEqual(route.from, { x: from.box.x + from.box.width, y: from.box.y + out.top + ROW_HEIGHT / 2 }, `${route.wire.id} leaves its pin`);
    assert.deepEqual(route.to, { x: to.box.x, y: to.box.y + into.top + ROW_HEIGHT / 2 }, `${route.wire.id} lands on its pin`);
    assert(route.path.startsWith(`M ${route.from.x} ${route.from.y}`) && route.path.endsWith(`${route.to.x} ${route.to.y}`));
  }
}

async function main() {
  assert.equal(blueprintTriggerSummary({ type: "schedule", every: { lit: "1800s" } }), "Every 1800s");
  assert.equal(blueprintTriggerSummary({ type: "input", kind: { lit: "audio" } }), "Input: audio");
  assert.equal(blueprintTriggerSummary({ type: "event", kind: { lit: "tab_output" } }), "Event: tab_output");
  assert.equal(heldReasonLabel("uncertain_pipeline_result"), "the result delivery could not be confirmed");
  const pipeline = viewerPipeline();
  const build = (value: PipelineView = pipeline, expansion: ReadonlySet<string> = new Set()) => buildIncrementally(buildBlueprintGraph(value, expansion), async () => {}, new AbortController().signal);
  for (const reason of ["command_failed", "deadline"]) {
    const failed = { ...pipeline, state: "failed", nodes: [{ ...pipeline.nodes[0], key: "step-0", kind: "step" as const,
      state: "failed", reason, resultSummary: reason }], nested_run: { id: pipeline.runId!, nodes: [
      { step_index: 0, step_name: "probe", state: "done", outcome: "unchanged" },
    ] } };
    const rows = blueprintJournalNodes(failed);
    assert.equal(rows[0].state, "failed");
    assert.equal(rows[0].reason, reason);
    assert.equal(blueprintPipelineOutcome(failed)?.reason, reason);
    const graph = await build(failed);
    assert.equal(graph.nodes.find((node) => node.id === "probe")?.evidence?.state, "failed");
    assert.equal(graph.nodes.find((node) => node.id === "probe")?.evidence?.journal?.reason, reason);
    assert.equal(failed.nested_run.nodes[0].state, "done");
    const foreign = { ...failed, nested_run: { ...failed.nested_run, nodes: [{ ...failed.nested_run.nodes[0], run_id: "other" }] } };
    assert.equal(blueprintJournalNodes(foreign)[0].state, "done");
  }

  const empty = new Set<string>();
  const expanded = toggleBlueprintExpansion(empty, key([], "invoke"));
  assert.equal(empty.size, 0);
  assert.equal(toggleBlueprintExpansion(expanded, key([], "invoke")).size, 0);
  const run = pipeline.runs![0].pipeline;
  const collapsed = await build(pipeline);
  assert(!collapsed.nodes.some((node) => node.id === "return-count"));
  assert(collapsed.wires.some((wire) => wire.kind === "exec" && wire.from.node === key([], "probe") && wire.from.pin === "route:done" && wire.to.node === key([], "invoke")));
  assert(collapsed.wires.some((wire) => wire.kind === "data" && wire.from.node === key([], "probe") && wire.to.node === key([], "invoke") && wire.to.pin === "bind:count"), "call bindings draw a typed data wire");
  assert(!collapsed.wires.some((wire) => wire.from.node === key([], "probe") && wire.to.node === key([], "skipped")));
  const probe = collapsed.nodes.find((node) => node.id === "probe")!;
  assert.deepEqual(probe.outputs.filter((pin) => pin.kind === "exec").map((pin) => pin.label), ["done", "failed"], "one labelled exec pin per outcome");
  assert.equal(probe.type, "code");
  assert.equal(collapsed.nodes.find((node) => node.id === "invoke")?.type, "call");
  assert.deepEqual(collapsed.nodes.find((node) => node.id === "invoke")?.callee, { name: "mem:helper", openable: true }, "a call node names its sub-graph and knows it can open");
  assert.equal(probe.callee, undefined);
  assert.equal(collapsed.mode, "run");

  const graph = await build(run, new Set(expanded));
  const child = graph.nodes.find((node) => node.key === key(["invoke"], "return-count"));
  assert(child);
  assert.deepEqual(child.definition?.out, { type: "integer" });
  assert.equal(child.evidence?.journal?.reason, "contract_violation");
  assert.equal(child.evidence?.journal?.call_stack?.[0].hash, call.pin.hash);
  assert(graph.wires.some((wire) => wire.from.node === key([], "invoke") && wire.from.pin === "callee" && wire.to.node === key(["invoke"], "return-count")), "an expanded call wires into its callee");
  assert.equal(graph.scopes.find((scope) => scope.call === key([], "invoke"))?.title, call.pin.id);
  assert.equal(blueprintPipelineOutcome(run)?.step_name, "return-count");
  assert.equal(blueprintPipelineOutcome(pipeline), null);
  assert.equal(blueprintPipelineOutcome({ ...run, nested_run: pipeline.nested_run }), null);
  const mismatch = await build({ ...run, nested_run: pipeline.nested_run }, new Set(expanded));
  assert.equal(mismatch.mode, "unavailable");
  assert(mismatch.nodes.every((node) => node.evidence === null), "a run without matching evidence never shows per-node states");
  const layout = layoutBlueprint(graph);
  assertGeometry(graph, layout);
  assert.equal(layout.frames.length, 1, "the expanded callee sits in its own frame");
  const frame = layout.frames[0].box;
  const childBox = layout.byKey.get(child.key)!.box;
  assert(childBox.x >= frame.x && childBox.y >= frame.y && childBox.x + childBox.width <= frame.x + frame.width && childBox.y + childBox.height <= frame.y + frame.height);

  const fields = blueprintDiffModel(diff).map((row) => row.field);
  for (const field of ["node", "routes", "out", "caps", "code", "imports", "call", "in", "inputs", "result"]) assert(fields.includes(field));
  assert(fields.some((field) => field.startsWith("call pin:")));
  const repeated = { ...run, call_graph: { ...document, steps: [document.steps[2]] }, expanded_calls: [{ ...call, graph: { ...call.graph, steps: [{ ...call.graph.steps[0], id: "invoke" }] } }],
    nested_run: { id: run.runId!, nodes: [
      { step_index: 1, parent_step_index: 0, step_name: "invoke", state: "failed", reason: "contract_violation", call_stack: [{ node: "invoke", ...call.pin }] },
      { step_index: 0, parent_step_index: null, step_name: "invoke", state: "done", call_stack: [{ node: "invoke", ...call.pin }] },
    ] } };
  assert.equal(blueprintPipelineOutcome({ ...repeated, nested_run: { ...repeated.nested_run, nodes: [...repeated.nested_run.nodes].reverse().map((row) => ({ ...row, state: "failed" })) } })?.step_index, 1);
  assert.equal(blueprintPipelineOutcome({ ...run, state: "idle" }), null);
  const repeatedGraph = await build(repeated, new Set(expanded));
  assert.equal(repeatedGraph.nodes.find((node) => node.key === key([], "invoke"))?.evidence?.state, "done");
  assert.equal(repeatedGraph.nodes.find((node) => node.key === key(["invoke"], "invoke"))?.evidence?.state, "failed");
  const old = await build({ ...repeated, nested_run: { ...repeated.nested_run, nodes: [repeated.nested_run.nodes[0]].map((row) => ({ ...row, parent_step_index: null })) } }, new Set(expanded));
  assert(!old.nodes.find((node) => node.key === key(["invoke"], "invoke"))?.evidence?.journal);
  const unavailable = await build({ ...run, historical_graph_available: false }, new Set(expanded));
  assert.deepEqual(unavailable.nodes, []);
  const wrongVersion = await build({ ...run, graph_definition_version: 2, nested_run: { ...run.nested_run!, definition_version: 1 } }, new Set(expanded));
  assert(!wrongVersion.nodes.some((node) => node.evidence?.journal));
  const retained = await build({ ...run, graph_definition_version: 1, nested_run: { ...run.nested_run!, definition_version: 1, nodes: run.nested_run!.nodes.map((row) => ({ ...row, run_id: run.runId!, definition_version: 1 })) } }, new Set(expanded));
  assert.equal(retained.nodes.find((node) => node.key === key(["invoke"], "return-count"))?.evidence?.journal?.reason, "contract_violation");
  const afterCall = await build({ ...run, graph_definition_version: 1,
    call_graph: { ...document, steps: [...document.steps, { id: "after-call", run: { lit: "printf final" } }] },
    nested_run: { id: run.runId!, definition_version: 1, nodes: [
      { step_index: 4, parent_step_index: null, definition_version: 1, step_name: "after-call", state: "done" },
    ] } });
  assert.equal(afterCall.nodes.find((node) => node.id === "after-call")?.evidence?.state, "done");
  const unproven = await build({ ...run, nodes: run.nodes.map((node) => ({ ...node, state: "failed", resultSummary: "unrelated marker" })), nested_run: { id: run.runId!, nodes: [] } });
  assert.equal(unproven.mode, "unavailable");
  assert(unproven.nodes.every((node) => node.evidence === null), "unproven runs say evidence is unavailable instead of waiting");
  const finished = await build({ ...run, state: "idle", nested_run: { id: run.runId!, nodes: [
    { step_index: -1, parent_step_index: null, step_name: "$run", state: "done" },
    { step_index: -1, parent_step_index: null, step_name: "$result", state: "done", result_summary: "result delivered" },
    { step_index: 0, parent_step_index: null, step_name: "probe", state: "done", outcome: "done" },
    { step_index: 2, parent_step_index: null, step_name: "invoke", state: "done" },
    { step_index: 3, parent_step_index: 2, step_name: "return-count", state: "held", reason: "uncertain_pipeline_claim", call_stack: [{ node: "invoke", ...call.pin }] },
  ] } }, new Set(expanded));
  const state = (path: string[], id: string) => finished.nodes.find((node) => node.key === key(path, id))?.evidence;
  assert.equal(state([], "$trigger")?.state, "done");
  assert.equal(state([], "$result")?.summary, "result delivered");
  assert.equal(state(["invoke"], "return-count")?.summary, "uncertain_pipeline_claim");
  assert.equal(state([], "skipped")?.state, "not reached", "a finished run marks untouched nodes as not reached, never waiting");
  assert(!finished.nodes.some((node) => node.evidence?.state === "waiting"));
  const lit = litWires(finished);
  assert(lit.has(finished.wires.find((wire) => wire.from.node === key([], "probe") && wire.from.pin === "route:done")!.id), "the taken route lights");
  assert(!lit.has(finished.wires.find((wire) => wire.from.node === key([], "probe") && wire.from.pin === "route:failed")!.id), "routes not taken stay dark");
  const labels = finished.nodes.map((node) => nodeLabel(node));
  assert.equal(new Set(labels).size, labels.length, `duplicate node labels: ${labels.join(", ")}`);
  assert(labels.includes("Result in invoke"));

  const http = await build({ ...pipeline, call_graph: { ...document, steps: [{ id: "health", uses: "http" }, { id: "ask", uses: "ask", buckets: ["ready"] }] } });
  assert.deepEqual(http.wires.filter((wire) => wire.from.node === key([], "health")).map((wire) => wire.outcome), ["up", "down", "failed"]);
  assert.deepEqual(http.nodes.find((node) => node.id === "ask")?.outputs.map((pin) => pin.label), ["ready", "other"]);
  const fanIn = await build({ ...pipeline, nested_run: null, call_graph: { ...document, trigger: { type: "input", kind: { lit: "audio" } }, steps: [
    { id: "a", run: { lit: "a" }, in: { type: "string" }, routes: { failed: "recover" } },
    { id: "b", run: { lit: "b" }, routes: { failed: "recover" } },
    { id: "c", run: { lit: "c" }, routes: { failed: "recover", done: "merge" } },
    { id: "recover", run: { lit: "r" }, routes: { done: "merge" } },
    { id: "merge", run: { lit: "m" } },
  ] } });
  assert(fanIn.wires.some((wire) => wire.kind === "data" && wire.from.node === key([], "$trigger") && wire.to.pin === "data:in"), "an input trigger feeds the first typed port");
  assertGeometry(fanIn, layoutBlueprint(fanIn));
  const noted = layoutBlueprint(fanIn, { title: "Note", text: "Normalise audio into text", items: ["transcribe a call"], facts: ["2 uses · 1 success"] });
  assertGeometry(fanIn, noted);
  assert(noted.note, "a note is laid out");
  assert(noted.nodes.every((entry) => entry.box.y >= noted.note!.box.y + noted.note!.box.height), "the note sits above the whole graph");
  assert(noted.note.box.x + noted.note.box.width <= noted.bounds.width && noted.note.box.y + noted.note.box.height <= noted.bounds.height, "Fit all frames the note too");
  assert.deepEqual(noted.root, noted.note.box, "a readable opening starts at the note");
  assert.equal(layoutBlueprint(fanIn).note, null);

  const watch = capturedWatch as AutomationGraph;
  const runs = watchRuns(watch, watch.blueprints![0]);
  assert.equal(runs.length, 1);
  assert.deepEqual(runs[0].steps.map((step) => [step.step, step.state, step.outcome]), [["source", "done", null], ["decide", "done", "passed"], ["notify", "done", null]]);
  const watchPipeline = withRetainedRuns(pipelineModel(watch.blueprints![0]), runs);
  assert.equal(watchPipeline.runId, runs[0].id);
  const watchGraph = await build(watchPipeline);
  assert.equal(watchGraph.mode, "run");
  assert.deepEqual(watchGraph.nodes.filter((node) => node.definition).map((node) => [node.id, node.evidence?.state]), [["source", "done"], ["decide", "done"], ["notify", "done"]], "a retained watch run reaches its typed nodes");
  assert.equal(watchGraph.nodes.find((node) => node.id === "decide")?.type, "logic", "a pattern-only classifier is logic, not an agent");
  assert(watchGraph.wires.some((wire) => wire.from.node === key([], "source") && wire.from.pin === "next" && wire.to.node === key([], "decide")));
  assertGeometry(watchGraph, layoutBlueprint(watchGraph));

  const large = { ...pipeline, call_graph: { ...document, steps: Array.from({ length: 24 }, (_, index) => ({ ...document.steps[2], id: `call-${index}`, routes: { done: "@next", failed: "@stop" } })) },
    expanded_calls: Array.from({ length: 24 }, (_, index) => ({ ...call, node: `call-${index}` })), nested_run: null };
  const all = new Set(large.call_graph.steps.map((node) => key([], node.id)));
  let chunks = 0, longest = 0, last = performance.now();
  const started = last;
  const measured = await buildIncrementally(buildBlueprintGraph(large, all), async () => {
    longest = Math.max(longest, performance.now() - last); chunks++;
    await new Promise<void>((resolve) => { const channel = new MessageChannel(); channel.port1.onmessage = () => { channel.port1.close(); channel.port2.close(); resolve(); }; channel.port2.postMessage(null); });
    last = performance.now();
  }, new AbortController().signal);
  const elapsed = performance.now() - started;
  longest = Math.max(longest, performance.now() - last);
  const layoutStarted = performance.now();
  const largeLayout = layoutBlueprint(measured);
  const layoutMs = performance.now() - layoutStarted;
  assert(chunks > 0);
  assert.equal(measured.nodes.filter((node) => node.definition).length, 48);
  assertGeometry(measured, largeLayout);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(buildIncrementally(buildBlueprintGraph(large, all), async () => {}, controller.signal));
  console.log(`Blueprint graph model: passed; 24 root calls / 48 typed nodes; model=${elapsed.toFixed(3)}ms maxChunk=${longest.toFixed(3)}ms layout=${layoutMs.toFixed(3)}ms yields=${chunks}`);
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
