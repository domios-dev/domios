import assert from "node:assert/strict";
import { boxesOverlap } from "../Canvas/layout";
import { blueprintModel, MAX_ACTIONS_PER_STAGE, nodeKeys, wireId, type BlueprintNode } from "./blueprintModel";
import { ALL_HANDLER_TYPES, followIdentity, sourceStage, typedGraph, watchIdentity } from "./blueprintModel.testdata";
import { frameBlueprint, layoutBlueprint, READABLE_SCALE, ROW_HEIGHT } from "./blueprintLayout";
import { automationKey } from "./types";
import { inspectBlueprintNode } from "./blueprintInspector";

const tests: Array<[string, () => void]> = [];
function test(name: string, run: () => void) { tests.push([name, run]); }

const watch = automationKey(watchIdentity);
const follow = automationKey(followIdentity);
const byKey = (nodes: readonly BlueprintNode[], key: string) => {
  const found = nodes.find((node) => node.key === key);
  assert.ok(found, key);
  return found;
};

test("every stage explodes into Trigger, Source, decision and one action per declared handler", () => {
  const model = blueprintModel(typedGraph());
  assert.deepEqual(model.stages.map((stage) => stage.key), [watch, follow]);
  const keys = nodeKeys(watch);
  assert.deepEqual(model.stages[0].nodes.slice(0, 3), [keys.trigger, keys.source, keys.decision]);
  assert.deepEqual(model.nodes.filter((node) => node.stageKey === watch && node.type === "action").map((node) => node.action!.handler!.type), [
    "notify", "send", "cron", "command", "blueprint", "escalate", "respond_prompt", "stop",
  ]);
  assert.equal(new Set(model.nodes.map((node) => node.key)).size, model.nodes.length, "node keys are unique");
  assert.equal(model.nodes.filter((node) => node.stageKey === watch).length, 3 + ALL_HANDLER_TYPES.length);
  assert.ok(!model.nodes.some((node) => node.stage.identity.kind !== "blueprint"), "worker and cron targets are named on actions, never invented as stages");
});

test("the agent node carries prompt, receives pin and one exec pin per bucket with its route definition, never code", () => {
  const model = blueprintModel(typedGraph());
  const agent = byKey(model.nodes, nodeKeys(watch).decision);
  assert.equal(agent.type, "agent");
  assert.equal(agent.title, "Jev · jev-latest");
  assert.equal(agent.body?.kind, "prompt");
  assert.equal(agent.body?.text, typedGraph().nodes[0].detail!.definition!.question);
  assert.deepEqual(agent.inputs.map((pin) => [pin.id, pin.kind]), [["exec", "exec"], ["receives", "data"]]);
  assert.deepEqual(agent.outputs.map((pin) => [pin.bucket, pin.kind, pin.detail]), [
    ["green", "exec", undefined], ["red", "exec", undefined], ["flaky", "exec", undefined],
  ]);
  assert.deepEqual(agent.outputs.map((pin) => pin.connected), [false, true, true], "a bucket without actions is an open pin");
  const code = model.nodes.filter((node) => node.body?.kind === "code").map((node) => node.type);
  assert.deepEqual(code, ["source", "action"], "code appears only on Source and Command nodes");
});

test("source and decision connect with exec and data wires; the chain ends on the next stage's trigger", () => {
  const model = blueprintModel(typedGraph());
  const keys = nodeKeys(watch);
  const ids = new Set(model.wires.map((wire) => wire.id));
  for (const id of [wireId(keys.trigger, "then", keys.source), wireId(keys.source, "then", keys.decision), wireId(keys.source, "output", keys.decision)]) assert.ok(ids.has(id), id);
  assert.equal(model.wires.find((wire) => wire.id === wireId(keys.source, "output", keys.decision))?.kind, "data");
  const next = keys.action("flaky", "follow-up");
  const chain = model.wires.find((wire) => wire.id === wireId(next, "then", nodeKeys(follow).trigger));
  assert.ok(chain, "action pin wires to the chained stage trigger");
  assert.equal(chain.back, false);
  const loop = model.wires.find((wire) => wire.id === wireId(nodeKeys(follow).action("fixed", "loop-back"), "then", keys.trigger));
  assert.ok(loop?.back, "activation of an earlier stage is a loop wire");
  const rootTrigger = byKey(model.nodes, keys.trigger);
  assert.equal(rootTrigger.trigger, "schedule", "a loop back keeps the root's own trigger");
  assert.equal(rootTrigger.title, "Every 20 s");
  assert.deepEqual(rootTrigger.inputs.map((pin) => pin.id), ["activate"], "but gives the loop wire a pin to land on");
  const followTrigger = byKey(model.nodes, nodeKeys(follow).trigger);
  assert.equal(followTrigger.trigger, "activated");
  assert.deepEqual(followTrigger.inputs.map((pin) => pin.id), ["activate"]);
});

test("actions name their targets and only chaining actions get an exec out", () => {
  const model = blueprintModel(typedGraph());
  const keys = nodeKeys(watch);
  const title = (bucket: string, id: string) => byKey(model.nodes, keys.action(bucket, id)).title;
  assert.equal(title("red", "send-fixer"), "Send to Fixer");
  assert.equal(title("red", "start-cron"), "Start Nightly sweep");
  assert.equal(title("red", "rebuild"), "Run rebuild");
  assert.equal(title("flaky", "follow-up"), "Run Follow terminal");
  assert.equal(byKey(model.nodes, keys.action("red", "send-fixer")).subtitle, "claude · opus");
  assert.equal(byKey(model.nodes, keys.action("red", "rebuild")).body?.text, "cargo clean && cargo check");
  for (const node of model.nodes.filter((entry) => entry.type === "action")) {
    assert.deepEqual(node.inputs.map((pin) => [pin.id, pin.kind]), [["exec", "exec"], ["payload", "data"]]);
    assert.equal(node.outputs.length, node.action!.handler!.type === "blueprint" ? 1 : 0);
  }
});

test("every source kind maps to its node and trigger", () => {
  const cases = [
    { source: { type: "probe" }, extra: { cadence_ms: 20_000, detail: { definition: { probe: "date" } } }, kind: "probe", trigger: "schedule", title: "Run probe", trig: "Every 20 s", body: "code" },
    { source: { type: "terminal", kind: "tab", pattern: null, idle_ms: 1500 }, extra: {}, kind: "terminal", trigger: "event", title: "Terminal: a Team tab", trig: "On new activity", body: "text" },
    { source: { type: "observe", kind: "cron_run" }, extra: {}, kind: "observe", trigger: "event", title: "Observe: a cron run", trig: "On new activity", body: null },
    { source: { type: "input", kind: "audio", ref_type: "file", display_name: "memo.wav", convert_to: "text" }, extra: {}, kind: "input", trigger: "manual", title: "Input: audio", trig: "On input", body: "text" },
  ] as const;
  for (const entry of cases) {
    const graph = sourceStage(`stage-${entry.kind}`, { ...entry.source }, entry.extra);
    const model = blueprintModel(graph);
    const key = automationKey(graph.scenarios[0].root);
    const source = byKey(model.nodes, nodeKeys(key).source);
    const trigger = byKey(model.nodes, nodeKeys(key).trigger);
    assert.equal(source.source, entry.kind);
    assert.equal(source.title, entry.title);
    assert.equal(source.body?.kind ?? null, entry.body, `${entry.kind} body`);
    assert.equal(trigger.trigger, entry.trigger);
    assert.equal(trigger.title, entry.trig);
    assert.equal(byKey(model.nodes, nodeKeys(key).decision).type, "logic", "no classifier configured means a Logic node");
  }
});

test("pattern-only stages render a Logic node listing their rules with a configured fallback", () => {
  const model = blueprintModel(typedGraph());
  const logic = byKey(model.nodes, nodeKeys(follow).decision);
  assert.equal(logic.type, "logic");
  assert.equal(logic.title, "Pattern match");
  assert.equal(logic.body?.text, "fixed: /Finished/\nbroken: /error/");
});

test("definition controls decision type and command language", () => {
  const graph = typedGraph();
  graph.nodes = graph.nodes.map((stage, index) => index ? stage : { ...stage, detail: { definition: { ...stage.detail!.definition, command_language: "powershell", decision_kind: "pattern" } } });
  const model = blueprintModel(graph);
  assert.equal(byKey(model.nodes, nodeKeys(watch).decision).type, "logic");
  for (const node of model.nodes.filter((node) => node.stageKey === watch && node.body?.kind === "code")) {
    assert.equal(node.body?.language, "powershell");
    assert.equal(inspectBlueprintNode(node, graph).code?.language, "powershell");
  }
});

test("actions past the per-stage bound fold into one counted overflow node", () => {
  const graph = sourceStage("wide", { type: "probe" });
  graph.nodes[0].outcomes = [{ bucket: "done", handlers: Array.from({ length: MAX_ACTIONS_PER_STAGE + 3 }, (_, index) => ({ id: `n${index}`, type: "notify", target: null, availability: "available" as const, last_result: null })) }];
  const model = blueprintModel(graph);
  const overflow = model.nodes.find((node) => node.action?.overflow);
  assert.equal(overflow?.action?.overflow, 3);
  assert.equal(model.nodes.filter((node) => node.type === "action").length, MAX_ACTIONS_PER_STAGE + 1);
});

test("layout flows left to right, keeps nodes apart and ends every wire on its pin", () => {
  const model = blueprintModel(typedGraph());
  const layout = layoutBlueprint(model);
  const box = (key: string) => layout.byKey.get(key)!.box;
  const keys = nodeKeys(watch);
  assert.ok(box(keys.source).x > box(keys.trigger).x + box(keys.trigger).width);
  assert.ok(box(keys.decision).x > box(keys.source).x + box(keys.source).width);
  assert.ok(box(keys.action("red", "notify-owner")).x > box(keys.decision).x + box(keys.decision).width);
  assert.ok(box(nodeKeys(follow).trigger).x > box(keys.action("red", "notify-owner")).x, "chained stage sits after its activator");
  const boxes = layout.nodes.map((entry) => entry.box);
  boxes.forEach((a, index) => boxes.slice(index + 1).forEach((b) => assert.ok(!boxesOverlap(a, b), "nodes never overlap")));
  for (const frame of layout.frames) for (const key of model.stages.find((stage) => stage.key === frame.stageKey)!.nodes) {
    const node = box(key);
    assert.ok(node.x >= frame.box.x && node.y >= frame.box.y && node.x + node.width <= frame.box.x + frame.box.width && node.y + node.height <= frame.box.y + frame.box.height, "comment frame wraps its stage");
  }
  assert.equal(layout.wires.length, model.wires.length, "every wire is routed");
  for (const route of layout.wires) {
    const from = layout.byKey.get(route.wire.from.node)!;
    const row = from.rows.find((entry) => entry.right?.id === route.wire.from.pin)!;
    assert.equal(route.from.x, from.box.x + from.box.width);
    assert.equal(route.from.y, from.box.y + row.top + ROW_HEIGHT / 2);
    assert.ok(route.path.startsWith(`M ${route.from.x} ${route.from.y}`));
    assert.ok(route.path.endsWith(`${route.to.x} ${route.to.y}`));
    assert.ok(route.to.x <= layout.bounds.width && route.to.y <= layout.bounds.height);
  }
  const loop = layout.wires.find((route) => route.wire.back)!;
  assert.ok(/C .* C /.test(loop.path), "loop wires take two curves under the graph");
});

test("layout is deterministic and a result-only change keeps every position", () => {
  const first = layoutBlueprint(blueprintModel(typedGraph()));
  const changed = typedGraph();
  changed.nodes = changed.nodes.map((node, index) => index ? node : { ...node, last_result: { bucket: "red", accepted_at: 5, handlers: [] }, state: "running" });
  const second = layoutBlueprint(blueprintModel(changed));
  assert.deepEqual(second.nodes.map((entry) => [entry.node.key, entry.box]), first.nodes.map((entry) => [entry.node.key, entry.box]));
});

test("framing fits a small graph whole and opens a large one on the first stage at a readable scale", () => {
  const layout = layoutBlueprint(blueprintModel(typedGraph()));
  const wide = frameBlueprint(layout, { width: layout.bounds.width + 100, height: layout.bounds.height + 100 }, false)!;
  assert.equal(wide.scale, 1);
  const narrow = frameBlueprint(layout, { width: 640, height: 420 }, false)!;
  assert.equal(narrow.scale, READABLE_SCALE);
  const root = layout.root!;
  assert.ok(Math.abs(root.x * narrow.scale + narrow.x - 28) < 1e-6, "first stage frame starts at the padding");
  const whole = frameBlueprint(layout, { width: 640, height: 420 }, true)!;
  assert.ok(whole.scale < READABLE_SCALE, "the explicit overview may go below readable");
});

let failed = false;
for (const [name, run] of tests) {
  try { run(); console.log(`✓ ${name}`); } catch (error) { failed = true; console.error(`✗ ${name}`); console.error(error); }
}
if (failed) process.exitCode = 1;
else console.log(`${tests.length} blueprint model cases passed`);
