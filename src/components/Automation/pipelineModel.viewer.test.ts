import assert from "node:assert/strict";
import { performance } from "node:perf_hooks";
import { buildBlueprintGraph, nodeLabel } from "./blueprintGraphModel";
import { blueprintNodeKey, toggleBlueprintExpansion } from "./blueprintExpansion";
import { buildIncrementally } from "./blueprintIncremental";
import { blueprintDiffModel } from "./blueprintDiffModel";
import { blueprintPipelineOutcome, canStopPipelineRun } from "./blueprintPipelineOutcome";
import { layoutPipelineChain } from "./pipelineLayout";
import { call, document, diff, viewerPipeline } from "./blueprintViewer.testdata";

async function main() {
  const pipeline = viewerPipeline();
  const empty = new Set<string>();
  const expanded = toggleBlueprintExpansion(empty, blueprintNodeKey([], "invoke"));
  assert.equal(empty.size, 0);
  assert.equal(toggleBlueprintExpansion(expanded, blueprintNodeKey([], "invoke")).size, 0);
  const run = pipeline.runs![0].pipeline;
  const build = (value = pipeline, expansion = empty) => buildIncrementally(buildBlueprintGraph(value, expansion), async () => {}, new AbortController().signal);
  const collapsed = await build();
  assert(!collapsed.nodes.some((node) => node.definition?.id === "return-count"));
  assert(collapsed.edges.some((edge) => edge.from === blueprintNodeKey([], "probe") && edge.to === blueprintNodeKey([], "invoke") && edge.label === "done"));
  assert(!collapsed.edges.some((edge) => edge.from === blueprintNodeKey([], "probe") && edge.to === blueprintNodeKey([], "skipped")));
  const graph = await build(run, new Set(expanded));
  const child = graph.nodes.find((node) => node.key === blueprintNodeKey(["invoke"], "return-count"));
  assert(child);
  assert.deepEqual(child.definition?.out, { type: "integer" });
  assert.equal(child.journal?.reason, "contract_violation");
  assert.equal(child.journal?.call_stack?.[0].hash, call.pin.hash);
  assert.equal(blueprintPipelineOutcome(run)?.step_name, "return-count");
  assert.equal(blueprintPipelineOutcome(pipeline), null);
  assert(!canStopPipelineRun(pipeline, run));
  assert(canStopPipelineRun(pipeline, pipeline));
  assert.equal(blueprintPipelineOutcome({ ...run, nested_run: pipeline.nested_run }), null);
  const mismatch = await build({ ...run, nested_run: pipeline.nested_run }, new Set(expanded));
  assert(!mismatch.nodes.find((node) => node.definition?.id === "return-count")?.journal);
  const layout = layoutPipelineChain(graph.nodes, graph.edges);
  assert.equal(layout.edges.length, graph.edges.length);
  assert(layout.edges.some((edge) => edge.from === blueprintNodeKey([], "probe") && edge.to === blueprintNodeKey([], "invoke")));
  assert(layout.nodes.every((box) => box.x >= 0 && box.y >= 0 && box.x + box.width <= layout.width && box.y + box.height <= layout.height));
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
  assert.equal(repeatedGraph.nodes.find((node) => node.key === blueprintNodeKey([], "invoke"))?.journal?.state, "done");
  assert.equal(repeatedGraph.nodes.find((node) => node.key === blueprintNodeKey(["invoke"], "invoke"))?.journal?.state, "failed");
  const old = await build({ ...repeated, nested_run: { ...repeated.nested_run, nodes: [repeated.nested_run.nodes[0]].map((row) => ({ ...row, parent_step_index: null })) } }, new Set(expanded));
  assert(!old.nodes.find((node) => node.key === blueprintNodeKey(["invoke"], "invoke"))?.journal);
  const orphan = await build({ ...repeated, nested_run: { ...repeated.nested_run, nodes: [repeated.nested_run.nodes[0]] } }, new Set(expanded));
  assert(!orphan.nodes.some((node) => node.journal));
  const unavailable = await build({ ...run, historical_graph_available: false }, new Set(expanded));
  assert.deepEqual(unavailable, { nodes: [], edges: [] });
  const wrongVersion = await build({ ...run, graph_definition_version: 2, nested_run: { ...run.nested_run!, definition_version: 1 } }, new Set(expanded));
  assert(!wrongVersion.nodes.some((node) => node.journal));
  const retained = await build({ ...run, graph_definition_version: 1, nested_run: { ...run.nested_run!, definition_version: 1, nodes: run.nested_run!.nodes.map((row) => ({ ...row, run_id: run.runId!, definition_version: 1 })) } }, new Set(expanded));
  assert.equal(retained.nodes.find((node) => node.key === blueprintNodeKey(["invoke"], "return-count"))?.journal?.reason, "contract_violation");
  const afterCall = await build({ ...run, graph_definition_version: 1,
    call_graph: { ...document, steps: [...document.steps, { id: "after-call", run: { lit: "printf final" } }] },
    nested_run: { id: run.runId!, definition_version: 1, nodes: [
      { step_index: 4, parent_step_index: null, definition_version: 1, step_name: "after-call", state: "done" },
    ] } });
  assert.equal(afterCall.nodes.find((node) => node.definition?.id === "after-call")?.journal?.state, "done");
  const unproven = await build({ ...run, nodes: run.nodes.map((node) => ({ ...node, state: "failed", resultSummary: "unrelated marker" })),
    nested_run: { id: run.runId!, nodes: [] } });
  assert(unproven.nodes.filter((node) => node.definition).every((node) => node.state === "waiting" && node.resultSummary === null));
  const finished = await build({ ...run, state: "idle", nested_run: { id: run.runId!, nodes: [
    { step_index: -1, parent_step_index: null, step_name: "$run", state: "done" },
    { step_index: -1, parent_step_index: null, step_name: "$result", state: "done", result_summary: "result delivered" },
    { step_index: 0, parent_step_index: null, step_name: "probe", state: "done" },
    { step_index: 2, parent_step_index: null, step_name: "invoke", state: "done" },
    { step_index: 3, parent_step_index: 2, step_name: "return-count", state: "held", reason: "uncertain_pipeline_claim", call_stack: [{ node: "invoke", ...call.pin }] },
  ] } }, new Set(expanded));
  assert.equal(finished.nodes.find((node) => node.key === blueprintNodeKey([], "$trigger"))?.state, "done");
  assert.equal(finished.nodes.find((node) => node.key === blueprintNodeKey([], "$result"))?.state, "done");
  assert.equal(finished.nodes.find((node) => node.key === blueprintNodeKey([], "$result"))?.resultSummary, "result delivered");
  assert.equal(finished.nodes.find((node) => node.key === blueprintNodeKey(["invoke"], "return-count"))?.resultSummary, "uncertain_pipeline_claim");
  const calleeControls = finished.nodes.filter((node) => node.kind !== "step" && node.callPath?.[0] === "invoke");
  assert(calleeControls.length >= 2);
  const labels = finished.nodes.map((node) => nodeLabel(node));
  assert.equal(new Set(labels).size, labels.length, `duplicate node labels: ${labels.join(", ")}`);
  assert(nodeLabel(calleeControls.find((node) => node.kind === "result")!).includes("invoke"));
  const http = await build({ ...pipeline, call_graph: { ...document, steps: [{ id: "health", uses: "http" }, { id: "ask", uses: "ask", buckets: ["ready"] }] } });
  assert.deepEqual(http.edges.filter((edge) => edge.from === blueprintNodeKey([], "health")).map((edge) => edge.label), ["up", "down", "failed"]);
  assert.deepEqual(http.edges.filter((edge) => edge.from === blueprintNodeKey([], "ask")).map((edge) => edge.label), ["ready", "other"]);
  const recoveredFailure = { ...run, nested_run: { id: run.runId!, nodes: [
    { entry_id: 20, step_index: 5, parent_step_index: null, step_name: "final", state: "failed", reason: "deadline" },
    { entry_id: 10, step_index: 0, parent_step_index: null, step_name: "earlier-call", state: "failed", reason: "contract_violation" },
    { entry_id: 9, step_index: 3, parent_step_index: 0, step_name: "earlier-leaf", state: "failed", reason: "contract_violation" },
  ] } };
  assert.equal(blueprintPipelineOutcome(recoveredFailure)?.step_name, "final");
  assert.equal(blueprintPipelineOutcome({ ...recoveredFailure, nested_run: { ...recoveredFailure.nested_run, nodes: [...recoveredFailure.nested_run.nodes].reverse() } })?.step_name, "final");
  const large = { ...pipeline, call_graph: { ...document, steps: Array.from({ length: 24 }, (_, index) => ({ ...document.steps[2], id: `call-${index}`, routes: { done: "@next", failed: "@stop" } })) },
    expanded_calls: Array.from({ length: 24 }, (_, index) => ({ ...call, node: `call-${index}` })), nested_run: null };
  const all = new Set(large.call_graph.steps.map((node) => blueprintNodeKey([], node.id)));
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
  const largeLayout = layoutPipelineChain(measured.nodes, measured.edges);
  const layoutMs = performance.now() - layoutStarted;
  assert(chunks > 0);
  assert.equal(measured.nodes.filter((node) => node.definition).length, 48);
  assert.equal(largeLayout.nodes.length, measured.nodes.length);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(buildIncrementally(buildBlueprintGraph(large, all), async () => {}, controller.signal));
  console.log(`pipelineModel viewer: passed; 24 root calls / 48 typed nodes; model=${elapsed.toFixed(3)}ms maxChunk=${longest.toFixed(3)}ms layout=${layoutMs.toFixed(3)}ms yields=${chunks}`);
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
