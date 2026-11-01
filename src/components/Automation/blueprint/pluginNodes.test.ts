import assert from "node:assert/strict";
import { blueprintNodeKey } from "../blueprintExpansion";
import { buildIncrementally } from "../blueprintIncremental";
import { pipelineModel } from "../pipelineModel";
import { buildBlueprintGraph } from "./graphModel";
import { frameBlueprint, layoutBlueprint } from "./layout";
import { computeFitTransform } from "@codeterm/canvas-camera";

import { document, pluginNodes } from "./pluginNodes.testdata";

async function main() {
  const pipeline = pipelineModel({ id: "plugins", name: document.name, state: "idle", run_id: null, current_step: null, last_event_at_ms: null,
    call_graph: document, expanded_calls: [], plugin_nodes: pluginNodes });
  const model = await buildIncrementally(buildBlueprintGraph(pipeline, new Set()), async () => {}, new AbortController().signal);
  const read = model.nodes.find((node) => node.key === blueprintNodeKey([], "read"))!;
  assert.equal(read.type, "plugin", "the payload's plugin mark chooses the kind");
  assert.equal(read.title, "Fixture: read page");
  assert.equal(read.kindLabel, "fixture");
  assert.equal(read.body?.text, "Reads one page");
  assert.deepEqual(read.plugin?.params, [["url", "https://example.com"], ["limit", "5"]]);
  assert.deepEqual(read.inputs.filter((pin) => pin.kind === "data").map((pin) => [pin.id, pin.type]), [["data:url", "text[0..1]"]]);
  assert.deepEqual(read.outputs.filter((pin) => pin.kind === "data").map((pin) => [pin.id, pin.type]), [["data:text", "text[1..1]"], ["data:shot", "image[0..1]"]]);
  assert.deepEqual(read.routes, [
    { outcome: "read", target: "use" }, { outcome: "busy", target: "$stop" },
    { outcome: "unknown", target: "$stop" }, { outcome: "canceled", target: "$stop" },
  ], "declared outcomes plus the host-reserved unknown and canceled");
  const data = model.wires.find((wire) => wire.kind === "data" && wire.to.node === blueprintNodeKey([], "use"))!;
  assert.deepEqual([data.from.pin, data.to.pin], ["data:text", "bind:in"], "a downstream binding lands on the declared output port");
  assert(read.outputs.find((pin) => pin.id === "data:text")!.connected);

  const gone = model.nodes.find((node) => node.key === blueprintNodeKey([], "gone"))!;
  assert.equal(gone.type, "plugin");
  assert.equal(gone.plugin?.available, false);
  assert.equal(gone.title, "Saved page reader");
  assert(!model.wires.some(wire => model.nodes.find(node => node.key === wire.to.node)?.control === "stop"), "stop routes never draw edges");
  assert(read.outputs.find(pin => pin.label === "busy")?.terminal, "stop outcomes terminate at their output port");

  const plain = await buildIncrementally(buildBlueprintGraph({ ...pipeline, plugin_nodes: undefined }, new Set()), async () => {}, new AbortController().signal);
  assert.equal(plain.nodes.find((node) => node.key === blueprintNodeKey([], "read"))!.type, "action", "without the payload mark a dotted id is not a plugin");
  const layout = layoutBlueprint(model);
  const initial = frameBlueprint(layout, {width:380,height:844}, false)!;
  assert(initial.scale >= 0.75, "a narrow initial view stays readable");
  const trigger = layout.nodes.find(entry => entry.node.control === "trigger")!.box;
  assert.equal((trigger.x + trigger.width / 2) * initial.scale + initial.x, 190, "the initial narrow view centres the trigger");
  assert(frameBlueprint(layout, {width:380,height:844}, true)!.scale >= 0.5, "narrow Fit all keeps its scale floor");
  assert.deepEqual(frameBlueprint(layout, {width:1280,height:900}, true),computeFitTransform(layout.bounds,1280,900,{maxScale:1,fitPadding:28}),"desktop Fit all is unchanged");
  console.log("Blueprint plugin nodes: typed pins, outcome routes and unavailable state come from payload descriptors");
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
