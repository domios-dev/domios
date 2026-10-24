import assert from "node:assert/strict";
import { buildIncrementally } from "../blueprintIncremental";
import { blueprintNodeKey } from "../blueprintExpansion";
import { viewerPipeline } from "../blueprintViewer.testdata";
import { buildBlueprintGraph } from "./graphModel";
import { pinValue, portName, previewText, PREVIEW_LIMIT, wireValue } from "./pinValues";

async function main() {
  const live = viewerPipeline();
  const run = live.runs![0].pipeline;
  const build = (pipeline: typeof run) => buildIncrementally(buildBlueprintGraph(pipeline, new Set()), async () => {}, new AbortController().signal);

  const summarised = await build(run);
  const probe = summarised.nodes.find((node) => node.key === blueprintNodeKey([], "probe"))!;
  const invoke = summarised.nodes.find((node) => node.key === blueprintNodeKey([], "invoke"))!;
  const out = probe.outputs.find((pin) => pin.kind === "data")!;
  const count = invoke.inputs.find((pin) => pin.id === "bind:count")!;
  assert.equal(portName(out), "out");
  assert.equal(portName(count), "count", "a bound input is named by its bind port");
  assert.deepEqual(pinValue(summarised, probe, out), { kind: "summary", text: "7" }, "without a recorded value, an output shows the run's summary");
  assert.deepEqual(pinValue(summarised, invoke, count), { kind: "none" }, "an input with nothing recorded says so");
  assert.equal(pinValue(summarised, probe, probe.outputs.find((pin) => pin.kind === "exec")!), null, "exec pins carry no value");
  const wire = summarised.wires.find((entry) => entry.kind === "data" && entry.to.pin === "bind:count")!;
  assert.deepEqual(wireValue(summarised, wire), { kind: "summary", text: "7" }, "a data wire carries what its source produced");

  const recorded = await build({ ...run, nested_run: { ...run.nested_run!, nodes: run.nested_run!.nodes.map((row) => row.step_name === "invoke"
    ? { ...row, ports: { inputs: { count: { preview: "7", type: "integer", bytes: 1 } }, outputs: { out: { preview: "{\"total\":8}", type: "integer", truncated: true } } } } : row) } });
  const invokeRecorded = recorded.nodes.find((node) => node.key === blueprintNodeKey([], "invoke"))!;
  assert.deepEqual(pinValue(recorded, invokeRecorded, invokeRecorded.inputs.find((pin) => pin.id === "bind:count")!), { kind: "value", value: { preview: "7", type: "integer", truncated: false, bytes: 1, redacted: false } });
  const produced = pinValue(recorded, invokeRecorded, invokeRecorded.outputs.find((pin) => pin.kind === "data")!);
  assert.equal(produced?.kind === "value" && produced.value.truncated, true);

  const definition = await build({ ...live, runId: null, nested_run: null });
  const definitionProbe = definition.nodes.find((node) => node.key === blueprintNodeKey([], "probe"))!;
  assert.equal(pinValue(definition, definitionProbe, definitionProbe.outputs.find((pin) => pin.kind === "data")!), null, "without a selected run, pins show no values");

  assert.equal(previewText("{\"a\":1}").text, "{\n  \"a\": 1\n}", "JSON previews read pretty");
  assert.equal(previewText("plain text").text, "plain text");
  const long = previewText("x".repeat(PREVIEW_LIMIT + 10));
  assert(long.cut && long.text.endsWith("…"), "long previews are cut for the popover");
  console.log("Pin values: recorded ports, run summaries, honest empty values, wires and previews");
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
