import assert from "node:assert/strict";
import type { BlueprintDocument, BlueprintPluginNode } from "../../../lib/blueprintApi";
import { blueprintNodeKey } from "../blueprintExpansion";
import { buildIncrementally } from "../blueprintIncremental";
import { pipelineModel } from "../pipelineModel";
import { buildBlueprintGraph } from "./graphModel";
import { layoutBlueprint } from "./layout";

const document: BlueprintDocument = {
  schema_version: 2, kind: "pipeline", name: "plugin-fixture",
  trigger: { type: "event", kind: { lit: "fixture" } },
  steps: [
    { id: "read", uses: "fixture.read", with: { url: { lit: "https://example.com" }, limit: { lit: 5 } }, routes: { read: "use", busy: "@stop" } },
    { id: "use", code: { lang: "python", src: "def main(value): return value" }, bind: { in: "read.out.text" } },
    { id: "gone", uses: "missing.step", routes: {} },
  ],
  result: { to: "owner" },
};
const pluginNodes: Record<string, BlueprintPluginNode> = {
  "fixture.read": {
    available: true, title: "Fixture: read page", plugin: "fixture", description: "Reads one page",
    inputs: { url: { kind: "text", minItems: 0, maxItems: 1 } },
    outputs: { text: { kind: "text", minItems: 1, maxItems: 1 }, shot: { kind: "image", minItems: 0, maxItems: 1 } },
    outcomes: ["read", "busy"],
  },
  "missing.step": { available: false },
};

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
  assert.match(gone.body?.text ?? "", /^Plugin unavailable/);
  assert.equal(gone.title, "gone");

  const plain = await buildIncrementally(buildBlueprintGraph({ ...pipeline, plugin_nodes: undefined }, new Set()), async () => {}, new AbortController().signal);
  assert.equal(plain.nodes.find((node) => node.key === blueprintNodeKey([], "read"))!.type, "action", "without the payload mark a dotted id is not a plugin");
  layoutBlueprint(model);
  console.log("Blueprint plugin nodes: typed pins, outcome routes and unavailable state come from payload descriptors");
}
void main().catch((error: unknown) => { console.error(error); process.exitCode = 1; });
